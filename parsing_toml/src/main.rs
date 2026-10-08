use winnow::ascii::{alphanumeric1, dec_uint, line_ending, space0};
use winnow::combinator::{cut_err, delimited, opt, preceded, separated_pair};
use winnow::error::{ContextError, StrContext, StrContextValue};
use winnow::prelude::*;

// ==========================================
// 1. The Delightful V-Style Core Trait
// ==========================================
pub trait FromCustomFormat: Sized {
    /// The linear parser engine
    fn parser(input: &mut &str) -> ModalResult<Self, ContextError>;

    /// The top-level user-facing API
    fn decode(input: &str) -> Result<Self, String> {
        Self::parser.parse(input).map_err(|err| {
            // winnow's ParseError Display already renders a contextual message
            format!("Config error:\n{}", err)
        })
    }
}

// ==========================================
// 2. Base Common Reusable Parsers
// ==========================================
pub fn parse_str_kv(
    key: &'static str,
) -> impl FnMut(&mut &str) -> ModalResult<String, ContextError> {
    move |input: &mut &str| {
        let (_, val) = separated_pair(
            key,
            (space0, '=', space0),
            delimited(
                '"',
                alphanumeric1,
                cut_err('"'),
            )
            .context(StrContext::Label("quoted string"))
            .context(StrContext::Expected(StrContextValue::CharLiteral('"'))),
        )
        .parse_next(input)?;
        Ok(val.to_string())
    }
}

pub fn parse_int_kv(
    key: &'static str,
) -> impl FnMut(&mut &str) -> ModalResult<u16, ContextError> {
    move |input: &mut &str| {
        let (_, val) = separated_pair(key, (space0, '=', space0), dec_uint)
            .context(StrContext::Label("unsigned integer"))
            .parse_next(input)?;
        Ok(val)
    }
}

// Eat optional surrounding whitespace / line endings between fields
fn eat_ws_nl(input: &mut &str) {
    let _ = (
        space0::<_, ContextError>,
        opt(line_ending::<_, ContextError>),
    )
        .parse_next(input)
        .ok();
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
            fn parser(input: &mut &str) -> ModalResult<Self, ContextError> {
                // Read and assign each field sequentially, just like V's compiler logic
                $(
                    let $field_name = $parser_func.parse_next(input)?;
                    // Consume line endings between parameters cleanly (optional at EOF)
                    let _ = $crate::eat_ws_nl(input);
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

custom_format_struct! {
    struct DbConfig {
        host: String = parse_str_kv("host"),
        port: u16 = parse_int_kv("port"),
    }
}

custom_format_struct! {
    struct Config {
        title: String = parse_str_kv("title"),
        secret_key: String = parse_str_kv("app_secret"),
        db: DbConfig = preceded(
            (space0, "[db]", space0, line_ending, space0),
            DbConfig::parser,
        ),
    }
}

fn main() {
    // ----------------------------------------------------
    // Scenario A: Happy Path
    // ----------------------------------------------------
    let valid_input = "title = \"MyApp\"\n\
         app_secret = \"supersecret\"\n\
         [db]\n\
         host = \"localhost\"\n\
         port = 5432\n";

    match Config::decode(valid_input) {
        Ok(config) => {
            println!("Successfully parsed!");
            println!("  Title: {}", config.title);
            println!("  Secret: {}", config.secret_key);
            println!("  DB Host: {}", config.db.host);
            println!("  DB Port: {}", config.db.port);
        }
        Err(err_msg) => println!("{}", err_msg),
    }

    // ----------------------------------------------------
    // Scenario B: Error Output (missing closing quote)
    // ----------------------------------------------------
    let broken_input = "title = \"MyApp\"\n\
         app_secret = \"supersecret\n\
         [db]\n\
         host = \"localhost\"\n\
         port = 5432\n";

    match Config::decode(broken_input) {
        Ok(config) => println!("Unexpectedly parsed: {:?}", config),
        Err(err_msg) => {
            println!("\n--- Error Test: missing closing quote ---");
            println!("{}", err_msg);
        }
    }

    // ----------------------------------------------------
    // Scenario C: Error Output (port is not a number)
    // ----------------------------------------------------
    let bad_port = "title = \"MyApp\"\n\
         app_secret = \"supersecret\"\n\
         [db]\n\
         host = \"localhost\"\n\
         port = five-thousand\n";

    match Config::decode(bad_port) {
        Ok(config) => println!("Unexpectedly parsed: {:?}", config),
        Err(err_msg) => {
            println!("\n--- Error Test: port is not a number ---");
            println!("{}", err_msg);
        }
    }

    // ----------------------------------------------------
    // Scenario D: Error Output (missing [db] section header)
    // ----------------------------------------------------
    let no_header = "title = \"MyApp\"\n\
         app_secret = \"supersecret\"\n\
         host = \"localhost\"\n\
         port = 5432\n";

    match Config::decode(no_header) {
        Ok(config) => println!("Unexpectedly parsed: {:?}", config),
        Err(err_msg) => {
            println!("\n--- Error Test: missing [db] header ---");
            println!("{}", err_msg);
        }
    }
}
