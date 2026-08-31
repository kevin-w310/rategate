use rategate::TokenBucket;
use std::collections::HashMap;
use std::io::{self, BufRead};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("simulate") => run_simulate(&args[1..]),
        Some("--help") | Some("-h") | None => print_usage(),
        Some(other) => {
            eprintln!("rategate: unknown command '{other}'");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn print_usage() {
    println!(
        "rategate simulate --capacity N --refill-rate N [--cost N] [--json]\n\
         \n\
         Reads one request per line from stdin, each either a bare\n\
         timestamp ('1.5') or a client id followed by a timestamp\n\
         ('alice 1.5'), and reports whether a token bucket with the given\n\
         parameters would allow it. Bare timestamps share a single\n\
         implicit 'default' bucket; each distinct client id gets its own\n\
         independent bucket with the same capacity, refill rate, and\n\
         cost, so several clients can be simulated in one run.\n\
         \n\
         Options:\n\
         \x20\x20--capacity N     bucket size in tokens (required)\n\
         \x20\x20--refill-rate N  tokens added per second (required)\n\
         \x20\x20--cost N         tokens each request consumes (default 1)\n\
         \x20\x20--json           emit a single JSON object instead of text"
    );
}

struct SimulateArgs {
    capacity: f64,
    refill_rate: f64,
    cost: f64,
    json: bool,
}

fn parse_simulate_args(args: &[String]) -> SimulateArgs {
    let mut capacity = None;
    let mut refill_rate = None;
    let mut cost = 1.0;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--capacity" => {
                capacity = Some(next_f64(args, &mut i, "--capacity"));
            }
            "--refill-rate" => {
                refill_rate = Some(next_f64(args, &mut i, "--refill-rate"));
            }
            "--cost" => {
                cost = next_f64(args, &mut i, "--cost");
            }
            "--json" => {
                json = true;
                i += 1;
            }
            other => {
                eprintln!("rategate: unrecognized argument '{other}'");
                std::process::exit(2);
            }
        }
    }

    let capacity = capacity.unwrap_or_else(|| {
        eprintln!("rategate: --capacity is required");
        std::process::exit(2);
    });
    let refill_rate = refill_rate.unwrap_or_else(|| {
        eprintln!("rategate: --refill-rate is required");
        std::process::exit(2);
    });

    SimulateArgs {
        capacity,
        refill_rate,
        cost,
        json,
    }
}

/// Reads the value for a `--flag value` pair and advances `i` past both.
fn next_f64(args: &[String], i: &mut usize, flag: &str) -> f64 {
    let value = args.get(*i + 1).unwrap_or_else(|| {
        eprintln!("rategate: {flag} requires a value");
        std::process::exit(2);
    });
    let parsed = value.parse().unwrap_or_else(|_| {
        eprintln!("rategate: {flag} value '{value}' is not a number");
        std::process::exit(2);
    });
    *i += 2;
    parsed
}

struct Outcome {
    client: String,
    t: f64,
    allowed: bool,
    remaining: f64,
}

fn run_simulate(args: &[String]) {
    let opts = parse_simulate_args(args);
    let mut buckets: HashMap<String, TokenBucket> = HashMap::new();
    let mut outcomes = Vec::new();

    for (line_no, line) in io::stdin().lock().lines().enumerate() {
        let line = line.unwrap_or_else(|err| {
            eprintln!("rategate: failed to read stdin: {err}");
            std::process::exit(1);
        });
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (client, t) = parse_request_line(line, line_no + 1);
        let bucket = buckets
            .entry(client.clone())
            .or_insert_with(|| TokenBucket::new(opts.capacity, opts.refill_rate));
        let allowed = bucket.try_acquire_at(t, opts.cost);
        outcomes.push(Outcome {
            client,
            t,
            allowed,
            remaining: bucket.tokens(),
        });
    }

    if opts.json {
        print_json(&opts, &outcomes);
    } else {
        print_text(&outcomes);
    }
}

/// Parses one line of stdin as either a bare timestamp, which shares the
/// implicit "default" bucket, or a "<client> <timestamp>" pair, which gets
/// its own independent bucket keyed by `client`.
fn parse_request_line(line: &str, line_no: usize) -> (String, f64) {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (client, raw_t) = match fields.as_slice() {
        [t] => ("default", *t),
        [client, t] => (*client, *t),
        _ => {
            eprintln!(
                "rategate: line {line_no}: expected '<timestamp>' or '<client> <timestamp>', got '{line}'"
            );
            std::process::exit(2);
        }
    };

    let t: f64 = raw_t.parse().unwrap_or_else(|_| {
        eprintln!("rategate: line {line_no}: '{raw_t}' is not a valid timestamp");
        std::process::exit(2);
    });

    (client.to_string(), t)
}

fn print_text(outcomes: &[Outcome]) {
    for o in outcomes {
        let verdict = if o.allowed { "ALLOW" } else { "DENY " };
        println!(
            "client={}  t={:.2}  {}  remaining={:.2}",
            o.client, o.t, verdict, o.remaining
        );
    }
    let allowed = outcomes.iter().filter(|o| o.allowed).count();
    println!(
        "{} requests: {} allowed, {} denied",
        outcomes.len(),
        allowed,
        outcomes.len() - allowed
    );
}

fn print_json(opts: &SimulateArgs, outcomes: &[Outcome]) {
    let allowed = outcomes.iter().filter(|o| o.allowed).count();

    let results: Vec<String> = outcomes
        .iter()
        .map(|o| {
            format!(
                "{{\"client\":\"{}\",\"t\":{:.2},\"allowed\":{},\"remaining\":{:.2}}}",
                json_escape(&o.client),
                o.t,
                o.allowed,
                o.remaining
            )
        })
        .collect();

    println!(
        "{{\"capacity\":{:.2},\"refill_rate\":{:.2},\"cost\":{:.2},\"results\":[{}],\"summary\":{{\"total\":{},\"allowed\":{},\"denied\":{}}}}}",
        opts.capacity,
        opts.refill_rate,
        opts.cost,
        results.join(","),
        outcomes.len(),
        allowed,
        outcomes.len() - allowed
    );
}

/// Escapes a string for embedding as a JSON string literal. Client ids come
/// from the caller's input, not a fixed set of values, so this can't assume
/// they're already JSON-safe.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_timestamp_uses_the_default_client() {
        assert_eq!(parse_request_line("1.5", 1), ("default".to_string(), 1.5));
    }

    #[test]
    fn client_and_timestamp_pair_uses_the_named_client() {
        assert_eq!(parse_request_line("alice 1.5", 1), ("alice".to_string(), 1.5));
    }

    #[test]
    fn surrounding_whitespace_between_fields_is_ignored() {
        assert_eq!(
            parse_request_line("  bob   2.0  ", 1),
            ("bob".to_string(), 2.0)
        );
    }

    #[test]
    fn json_escape_handles_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn json_escape_passes_through_plain_text() {
        assert_eq!(json_escape("alice"), "alice");
    }
}
