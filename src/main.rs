use rategate::TokenBucket;
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
         Reads one timestamp (seconds, as a float) per line from stdin,\n\
         each representing a request arriving at that time, and reports\n\
         whether a token bucket with the given parameters would allow it.\n\
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
    t: f64,
    allowed: bool,
    remaining: f64,
}

fn run_simulate(args: &[String]) {
    let opts = parse_simulate_args(args);
    let mut bucket = TokenBucket::new(opts.capacity, opts.refill_rate);
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
        let t: f64 = line.parse().unwrap_or_else(|_| {
            eprintln!(
                "rategate: line {}: '{}' is not a valid timestamp",
                line_no + 1,
                line
            );
            std::process::exit(2);
        });
        let allowed = bucket.try_acquire_at(t, opts.cost);
        outcomes.push(Outcome {
            t,
            allowed,
            remaining: bucket.available(),
        });
    }

    if opts.json {
        print_json(&opts, &outcomes);
    } else {
        print_text(&outcomes);
    }
}

fn print_text(outcomes: &[Outcome]) {
    for o in outcomes {
        let verdict = if o.allowed { "ALLOW" } else { "DENY " };
        println!("t={:.2}  {}  remaining={:.2}", o.t, verdict, o.remaining);
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
                "{{\"t\":{:.2},\"allowed\":{},\"remaining\":{:.2}}}",
                o.t, o.allowed, o.remaining
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
