use winnow::ascii::{alphanumeric1, dec_uint, line_ending, space0};
use winnow::combinator::{delimited, preceded, separated_pair, terminated};
use winnow::error::{ContextError, ParseError};
use winnow::prelude::*;

// ==========================================
// 1. The Delightful V-Style Core Trait
// ==========================================
pub trait FromCustomFormat: Sized {
    /// The linear parser engine
    fn parser<'s>(input: &mut &'s str) -> PResult<Self, ContextError>;

    /// The top-level user-facing API
    fn decode(input: &str) -> Result<Self, String> {
        let mut original_input = input;
        Self::parser(&mut original_input).map_err(|err| {
            // winnow provides an amazing standard formatting wrapper out of the box
            format!("Config error:\n{}", ParseError::format(&err, input))
        })
    }
}

// ==========================================
// 2. Base Common Reusable Parsers
// ==========================================
pub fn parse_str_kv<'s>(
    key: &'static str,
) -> impl FnMut(&mut &'s str) -> PResult<String, ContextError> {
    move |input: &mut &'s str| {
        let (_, val) = separated_pair(
            key,
            (space0, '=', space0),
            delimited('"', alphanumeric1, '"'),
        )
        .parse_next(input)?;
        Ok(val.to_string())
    }
}

pub fn parse_int_kv<'s, T>(
    key: &'static str,
) -> impl FnMut(&mut &'s str) -> PResult<T, ContextError>
where
    T: winnow::stream::AsChar + winnow::stream::ParseSlice<T> + num_traits::PrimInt,
{
    move |input: &mut &'s str| {
        let (_, val) = separated_pair(key, (space0, '=', space0), dec_uint).parse_next(input)?;
        Ok(val)
    }
}

// ==========================================
// 3. The Boilerplate Killer: The Declarative Macro
// ==========================================
#[macro_export]
macro_rules! custom_format_struct {
    (
        struct $struct_name:ident {
            $( $field_name:ident : $field_type:ty = $parser_func:expr ),* $(,)?
        }
    ) => {
        #[derive(Debug)]
        struct $struct_name {
            $( $field_name : $field_type ),*
        }

        impl FromCustomFormat for $struct_name {
            fn parser<'s>(input: &mut &'s str) -> PResult<Self, ContextError> {
                // Read and assign each field sequentially, just like V's compiler logic
                $(
                    let $field_name = $parser_func.parse_next(input)?;
                    // Consume line endings between parameters cleanly
                    let _ = (space0, line_ending, space0).parse_next(input)?;
                )*

                Ok($struct_name {
                    $( $field_name ),*
                })
            }
        }
    };
}

// ==========================================
// 4. Putting it together (The End User's perspective)
// ==========================================

// Define a sub-struct and its internal parsers
custom_format_struct! {
    struct DbConfig {
        host: String = parse_str_kv("host"),
        port: u16 = parse_int_kv("port"),
    }
}

// Define the root config struct
custom_format_struct! {
    struct Config {
        title: String = parse_str_kv("title"),
        secret_key: String = parse_str_kv("app_secret"),
        // Nested structural parsing handles itself automatically via the trait
        db: DbConfig = preceded((space0, "[db]", space0, line_ending, space0), DbConfig::parser),
    }
}

fn main() {
    // ----------------------------------------------------
    // Scenario A: Happy Path
    // ----------------------------------------------------
    let valid_input = r#"title = "MyApp"
app_secret = "supersecret"
[db]
host = "localhost"
port = 5432
"#;

    if let Ok(config) = Config::decode(valid_input) {
        println!("Successfully parsed! Title: {}", config.title);
        println!("Database Port: {}", config.db.port);
    }

    // ----------------------------------------------------
    // Scenario B: Delightful Error Output
    // ----------------------------------------------------
    let broken_input = r#"title = "MyApp"
app_secret = "supersecret
[db]
host = "localhost"
port = 5432
"#; // Notice the missing quote on line 2!

    if let Err(err_msg) = Config::decode(broken_input) {
        println!("\n--- Error Test Output ---");
        println!("{}", err_msg);
    }
}
