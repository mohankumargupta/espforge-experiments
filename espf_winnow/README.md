# Show Me: The ESPF Parser

Let me walk you through this code by **showing** what happens at each stage with concrete inputs and outputs.

---

## 1. What problem does this solve?

**Input** — a small text config format:
```
# espforge
  <project name="example project" description="..." />
  <chip type="esp32c3" />

# peripherals
  <gpio id="gpio4" pin="4" />
  <i2c id="i2c0" sda="8" scl="9" frequency="400kHz" />
```

**Output** — a flat `HashMap<String, String>`:
```
"espforge.project.name"           → "example project"
"espforge.project.description"    → "example showing espf file format"
"espforge.chip.type"              → "esp32c3"
"peripherals.gpio4.pin"           → "4"
"peripherals.i2c0.sda"            → "8"
"peripherals.i2c0.frequency"      → "400kHz"
```

Notice the key pattern: `section.node_id.key`. That `node_id` is the tag's `id="..."` attribute, or the tag name if no `id` exists (e.g., `<chip>` → `chip`).

**On failure** — a `ParseDiagnostic` that renders like:
```
error: Invalid attribute
  --> <source>:2:8
   |
 2 | <chip type "esp32c3" />
   |       ^^^^^^^^^^^^^^
```

---

## 2. The data types

```
┌─────────────────────────────────────────────────────┐
│ LeafTable                                           │
│   fields: HashMap<String, String>                   │
│           └─ "espforge.chip.type" → "esp32c3"       │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ EspForgeSection          (typed view over LeafTable)│
│   project_name, project_description,                │
│   chip_type, runtime_type                           │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Line<'a>          (per-line classification)         │
│   Section("espforge")     ← line was "# espforge"   │
│   Tag("chip type=...")    ← line was "<chip ..."    │
│   Ignore                  ← blank, "---", "-..."    │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ TagInfo<'a>       (parsed tag header)               │
│   name: "gpio"                                      │
│   id:   Some("gpio4")                               │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ ParseDiagnostic                                     │
│   source: String     (for rendering)                │
│   span:   Range<usize> (byte range to underline)    │
│   message: String                                   │
└─────────────────────────────────────────────────────┘
```

---

## 3. The main loop — `parse_to_leaf_table`

Think of it as a **state machine** with one piece of state: `section: Option<&str>`.

```
   source
     │
     ▼
┌──────────────────────────────────────────────────────────┐
│ input = LocatingSlice::new(source)                       │
│ table = {}                                               │
│ section = None                                           │
└──────────────────────────────────────────────────────────┘
     │
     ▼
   ┌───────────────────────────┐
   │ input empty?  ──yes──► return table
   └────────────┬──────────────┘
                no
                ▼
   ┌───────────────────────────────────────────────┐
   │ read one line + its byte span                 │
   │ ("# espforge", 0..11)                         │
   └───────────────┬───────────────────────────────┘
                   ▼
   ┌───────────────────────────────────────────────┐
   │ parse_line → Line::Section("espforge")        │
   └───────────────┬───────────────────────────────┘
                   ▼
   ┌───────────────────────────────────────────────┐
   │ match Line:                                   │
   │  Section → section = Some("espforge")         │
   │  Tag     → must have section (else error),    │
   │            insert_tag(...)                    │
   │  Ignore  → nothing                            │
   └───────────────┬───────────────────────────────┘
                   │
                   └──────► loop back
```

**Key insight:** `till_line_ending.with_span()` returns both the line *and* the byte range it occupied in the original source. That range is what makes the error highlights work later.

---

## 4. Line classification — `parse_line`

Given a raw line, decide what it is:

```
Input line                       → Output
──────────────────────────────────────────────────────
""                               → Ignore
"   "                            → Ignore
"--- ESPF file format"           → Ignore   ('-' prefix)
"# espforge"                     → Section("espforge")
"#   peripherals"                → Section("peripherals")
"  <chip type=\"esp32c3\" />"    → Tag("chip type=\"esp32c3\" />")
"???"                            → Ignore   (anything else)
```

The implementation is deliberately dumb:
```rust
let _ = space0.parse_next(&mut input);     // strip leading spaces
let Some(first) = input.chars().next() else { return Ok(Line::Ignore) };
match first {
    '#' => parse_section_tail(...),
    '<' => Ok(Line::Tag(...)),
    '-' => Ok(Line::Ignore),
    _   => Ok(Line::Ignore),
}
```

Only the *first non-space character* matters. No allocation, one pass.

---

## 5. The interesting part — `insert_tag`

This is where the code works hardest, and it uses a **two-pass** strategy.

### Why two passes?

Attributes look like `key="value"` but the order is arbitrary. We need to know the tag's `id` *before* we build the final key path `section.<id>.<key>`. Since the `id` attribute could appear anywhere, a single pass would either need to buffer attributes or do a lookup pass.

**Two passes** avoids both:
- **Pass 1:** validate + find `id` (borrows from `tag_tail`)
- **Pass 2:** emit fields into the owned table (borrows again from `tag_tail`)

No `Vec<(String, String)>` intermediate. No clones during scanning.

### Trace

Input: `tag_tail = "gpio id=\"gpio4\" pin=\"4\" />"`, `section = "peripherals"`

```
PASS 1 — parse_tag
─────────────────────────────────────────────────────────────
  parse_tag_name → "gpio"
  loop:
    skip whitespace
    see "id=\"gpio4\""
      parse_attribute → ("id", "gpio4")
      id = Some("gpio4")
    skip whitespace
    see "pin=\"4\""
      parse_attribute → ("pin", "4")    ← parsed but discarded
    skip whitespace
    see "/>"
      return TagInfo { name: "gpio", id: Some("gpio4") }
─────────────────────────────────────────────────────────────
  validate: first_pass is empty ✓

  node_id = "gpio4"

PASS 2 — walk from &tag_tail["gpio".len()..]  ← skip name, no re-parse
─────────────────────────────────────────────────────────────
  loop:
    skip whitespace → see "id=\"gpio4\""
      ("id", "gpio4") → key == "id" → continue (skip)
    skip whitespace → see "pin=\"4\""
      insert_field(fields, "peripherals", "gpio4", "pin", "4")
        → fields["peripherals.gpio4.pin"] = "4"
    skip whitespace → see "/>"
      break
─────────────────────────────────────────────────────────────
  second_pass empty ✓

  Result: {"peripherals.gpio4.pin": "4"}
```

### Important subtlety

Notice the `id` attribute is **skipped during insertion** in pass 2:
```rust
if key == "id" { continue; }
```
That's because `id` is already baked into the path as `node_id`. We don't want `peripherals.gpio4.id = "gpio4"`.

### The efficiency win

The refactored version writes:
```rust
let mut second_pass = &tag_tail[info.name.len()..];
```

In the original, pass 2 re-parsed the tag name with `parse_tag_name`. Now we just slice past it — the name was already parsed and validated in pass 1. O(1) instead of another scan.

---

## 6. The low-level parsers

These are the workhorse `winnow` combinators. All of them borrow `&'a str` slices — **zero allocation**.

```
parse_tag_name           take_while(1.., is_tag_name_char)
                         "gpio" → "gpio"

parse_attribute          key = take_while(1.., is_attribute_key_char)
                         then '=' then quoted value
                         "pin=\"4\"" → ("pin", "4")

parse_quoted_value       delimited('"', take_till(0.., '"'), '"')
                         "\"esp32c3\"" → "esp32c3"

skip_tag_whitespace      space0 (infallible)
```

The character predicates are extracted as named functions for readability:

```rust
fn is_tag_name_char(ch) -> bool {
    !ch.is_whitespace() && !matches!(ch, '/' | '>' | '=' | '"' | '<')
}

fn is_attribute_key_char(ch) -> bool {
    !ch.is_whitespace() && !matches!(ch, '=' | '/' | '>' | '"' | '<')
}
```

The difference: a tag *name* can't contain `=` (that would make it an attribute), but an attribute *key* also can't contain `=` (it's the separator).

---

## 7. Error handling — how a diagnostic flows

Say the input is:
```
# espforge
<chip type "esp32c3" />
```

Line 2 fails: `type "esp32c3"` is missing the `=`.

```
parse_attribute fails
        │
        ▼
insert_tag maps error:  "Invalid attribute in <chip>: ..."
        │
        ▼
parse_to_leaf_table wraps with ParseDiagnostic::new(
    source,
    line_span,      ← the byte range of line 2
    message,
)
        │
        ▼
ParseDiagnostic { source, span, message }
        │
        ▼ render()
┌─────────────────────────────────────────┐
│ error: Invalid attribute in <chip>: ... │
│   --> <source>:2:8                      │
│    |                                    │
│  2 | <chip type "esp32c3" />            │
│    |       ^^^^^^^^^^^^^^               │
└─────────────────────────────────────────┘
```

### The `non_empty_span` helper

`annotate_snippets` draws invisible annotations for empty spans. So `non_empty_span` widens a zero-width span:

```
span.start == span.end?

  yes, start < source.len()?
       yes → highlight the next char (the newline or first char of next line)
       no  → highlight the last char before EOF
```

This is why end-of-file errors still get a visible caret.

---

## 8. Data flow summary — one picture

```
     source: &str
         │
         ▼
  ┌──────────────────┐
  │ parse_to_leaf_   │  loop over lines
  │ table            │  keep `section` state
  └────────┬─────────┘
           │
     per line
           ▼
  ┌──────────────────┐
  │ parse_line       │  '#' → Section
  └────────┬─────────┘  '<' → Tag
           │            '-' → Ignore
           │            _   → Ignore
           ▼
  ┌──────────────────┐
  │ insert_tag       │  pass 1: parse_tag   → TagInfo
  │  (Tag only)      │  pass 2: insert_field → LeafFields
  └────────┬─────────┘
           │
           ▼
     LeafTable { fields: HashMap }

           │
           │  (optional)
           ▼
  EspForgeSection::extract_from(&table)
           │
           ▼
  { project_name, project_description, chip_type, runtime_type }
```

---

## 9. What changed in the refactor (readability wins)

| Before | After | Why |
|---|---|---|
| `dispatch! { any; ... }` for line classification | plain `match first_char` | easier to read, same cost |
| `space0.parse_next(...).map_err(...)` everywhere | `let _ = space0.parse_next(...)` | `space0` is infallible; the `Result` was noise |
| `skip_tag_whitespace` returned `Result<(), String>` | returns `()` | same reason |
| Inline closures in `take_while` | `is_tag_name_char`, `is_attribute_key_char` | named predicates read better |
| Pass 2 re-parsed tag name via `parse_tag_name` | `&tag_tail[info.name.len()..]` | avoids a redundant scan |
| `winnow::combinator::delimited(...)` inline path | `use` it, call `delimited(...)` | shorter call sites |

**Performance was not sacrificed** — the two-pass borrowed strategy is intact, no new allocations were introduced, and one redundant parse was removed.

---

## 10. Try it yourself

Feed this to `parse_to_leaf_table` and predict the output before running:

```
# peripherals
  <gpio id="gpio4" pin="4" />
  <gpio pin="5" />
```

**Answer:**
```
peripherals.gpio4.pin → "4"
peripherals.gpio.pin  → "5"    ← no id ⇒ node_id = tag name
```

The second `<gpio>` has no `id`, so `node_id = "gpio"`. That's the `info.id.unwrap_or(info.name)` line. And if you add a *third* `<gpio pin="6" />`, you'll get a `Duplicate field path` error — because it would also map to `peripherals.gpio.pin`.
