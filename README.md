# rategate

A token bucket rate limiter: a small Rust library plus a CLI to simulate one
against a timeline of request arrivals.

## The problem

You have some resource (an API, a queue, a downstream service) that can only
absorb a certain rate of work before it falls over or gets throttled by a
provider. Clients don't arrive at a steady rate — they burst. A rate limiter
needs to allow a reasonable burst without letting the average rate creep past
what the resource can handle.

A token bucket does this with two numbers: a capacity (how big a burst is
allowed) and a refill rate (the sustained rate you're willing to allow). The
bucket starts full. Every request costs some number of tokens; if there
aren't enough, the request is rejected. Tokens regenerate continuously over
time, capped at capacity. No queueing, no timers to manage — just "do I have
enough tokens right now."

## Library usage

```rust
use rategate::TokenBucket;

// burst of 10, sustained at 5 requests/sec
let mut bucket = TokenBucket::new(10.0, 5.0);

if bucket.try_acquire(1.0) {
    // handle the request
} else {
    // reject it, e.g. respond 429
}
```

`try_acquire` uses the wall clock. There's also `try_acquire_at(now, cost)`,
which takes the current time as an explicit `f64` number of seconds — this is
what lets the bucket be tested and simulated deterministically instead of
depending on real elapsed time, and it's what the CLI below is built on.

There's also a sliding window limiter in `rategate::sliding_window`, for
cases where you need an exact "never more than N requests in any window of
T seconds" guarantee rather than a token bucket's continuously refilling
balance:

```rust
use rategate::sliding_window::SlidingWindowLimiter;

// at most 100 requests in any trailing 60 second window
let mut limiter = SlidingWindowLimiter::new(100, 60.0);

if limiter.try_acquire() {
    // handle the request
} else {
    // reject it
}
```

The tradeoff against `TokenBucket` is memory: a sliding window limiter keeps
one timestamp per request still inside the window, so it's `O(limit)` rather
than `O(1)`. In exchange, a burst that used up capacity right at the start
of a window can't buy that capacity back until the window has fully passed
— there's no gradual refill to take advantage of.

## CLI usage

The `simulate` subcommand reads a list of request arrival times (one
floating point number of seconds per line) from stdin and reports, for each
one, whether a bucket with the given parameters would have allowed it.

```
$ printf '0.0\n0.1\n0.2\n0.3\n0.4\n0.5\n1.5\n' | rategate simulate --capacity 3 --refill-rate 1
t=0.00  ALLOW  remaining=2.00
t=0.10  ALLOW  remaining=1.10
t=0.20  ALLOW  remaining=0.20
t=0.30  DENY   remaining=0.30
t=0.40  DENY   remaining=0.40
t=0.50  DENY   remaining=0.50
t=1.50  ALLOW  remaining=0.50
7 requests: 4 allowed, 3 denied
```

The same run with `--json` for feeding into another tool or a dashboard:

```
$ printf '0.0\n0.1\n0.2\n0.3\n0.4\n0.5\n1.5\n' | rategate simulate --capacity 3 --refill-rate 1 --json
{"capacity":3.00,"refill_rate":1.00,"cost":1.00,"results":[{"t":0.00,"allowed":true,"remaining":2.00},{"t":0.10,"allowed":true,"remaining":1.10},{"t":0.20,"allowed":true,"remaining":0.20},{"t":0.30,"allowed":false,"remaining":0.30},{"t":0.40,"allowed":false,"remaining":0.40},{"t":0.50,"allowed":false,"remaining":0.50},{"t":1.50,"allowed":true,"remaining":0.50}],"summary":{"total":7,"allowed":4,"denied":3}}
```

Options:

- `--capacity N` — bucket size in tokens (required)
- `--refill-rate N` — tokens regenerated per second (required)
- `--cost N` — tokens each request consumes (default `1`)
- `--json` — emit the single JSON object shown above instead of text

## Building

Standard library only, no dependencies to fetch:

```
cargo build --release
cargo test
```
