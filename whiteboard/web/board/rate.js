// Pure pacing for page -> plugin messages. The host drops anything above 60 messages per second,
// so the page stays below that: edits queue up, live traffic (cursor, live stroke, laser, view)
// is dropped when the budget is used, because the next update replaces it anyway.

export class TokenBucket {
  constructor(rate, burst, now) {
    this.rate = rate;
    this.burst = burst;
    this.tokens = burst;
    this.last = now;
  }

  refill(now) {
    const dt = Math.max(0, now - this.last) / 1000;
    this.tokens = Math.min(this.burst, this.tokens + dt * this.rate);
    this.last = now;
  }

  take(now) {
    this.refill(now);
    if (this.tokens >= 1) {
      this.tokens -= 1;
      return true;
    }
    return false;
  }

  /** Milliseconds until the next token is available. */
  wait(now) {
    this.refill(now);
    return this.tokens >= 1 ? 0 : Math.ceil(((1 - this.tokens) / this.rate) * 1000);
  }
}

/**
 * Outgoing queue on top of a token bucket. `deliver` gets each message that may go out now;
 * `pump(now)` returns how long to wait before calling it again (0 = nothing queued).
 */
export class Pacer {
  constructor(deliver, rate, burst, now) {
    this.deliver = deliver;
    this.bucket = new TokenBucket(rate, burst, now);
    this.queue = [];
  }

  /** Ephemeral messages are dropped instead of queued when the budget is used up. */
  send(msg, ephemeral, now) {
    if (this.queue.length === 0 && this.bucket.take(now)) {
      this.deliver(msg);
      return true;
    }
    if (ephemeral) return false;
    this.queue.push(msg);
    return true;
  }

  pump(now) {
    while (this.queue.length > 0 && this.bucket.take(now)) this.deliver(this.queue.shift());
    return this.queue.length > 0 ? Math.max(1, this.bucket.wait(now)) : 0;
  }
}
