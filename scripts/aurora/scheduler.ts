/** Elapsed simulation seconds since the previous invocation or scheduling. */
export type ScheduledCallback = (elapsed: number) => void;

class Timer {
  readonly callback: ScheduledCallback;
  interval: number;
  invocations: number;
  untilNext: number;
  elapsed = 0;
  delaying: boolean;
  active = true;

  constructor(callback: ScheduledCallback, interval: number, repeat: number, delay: number) {
    this.callback = callback;
    this.interval = interval;
    this.invocations = repeat === Infinity ? -1 : repeat + 1;
    this.untilNext = delay > 0 ? delay : interval;
    this.delaying = delay > 0;
  }
}

/** Timers owned by one component and advanced at a simulation phase boundary. */
export class Scheduler {
  private timers: Timer[] = [];

  schedule(callback: ScheduledCallback, interval: number, repeat: number, delay: number): void {
    if (!Number.isFinite(interval) || interval < 0 || !Number.isFinite(delay) || delay < 0) {
      throw new Error("Schedule interval and delay must be finite and nonnegative");
    }
    if (repeat !== Infinity && (!Number.isSafeInteger(repeat) || repeat < 0 || repeat >= Number.MAX_SAFE_INTEGER)) {
      throw new Error("Repeat must be a nonnegative integer or Infinity");
    }

    for (const timer of this.timers) {
      if (timer.callback === callback) {
        if (!timer.delaying) timer.untilNext += interval - timer.interval;
        timer.interval = interval;
        return;
      }
    }
    this.timers.push(new Timer(callback, interval, repeat, delay));
  }

  unschedule(callback: ScheduledCallback): void {
    for (let index = 0; index < this.timers.length; index++) {
      const timer = this.timers[index];
      if (timer.callback === callback) {
        timer.active = false;
        this.timers.splice(index, 1);
        return;
      }
    }
  }

  clear(): void {
    for (const timer of this.timers) timer.active = false;
    this.timers = [];
  }

  tick(dt: number): void {
    // New timers start on the next phase. Canceled timers in this snapshot are skipped.
    for (const timer of this.timers.slice()) {
      if (!timer.active) continue;
      timer.elapsed += dt;
      timer.untilNext -= dt;
      if (timer.untilNext > 1e-9) continue;

      const elapsed = timer.elapsed;
      const callback = timer.callback;
      timer.elapsed = 0;
      timer.delaying = false;
      // Preserve fractional time without invoking a timer more than once per tick.
      const overdue = Math.max(0, -timer.untilNext);
      timer.untilNext = timer.interval > 0 ? timer.interval - (overdue % timer.interval) : 0;
      if (timer.invocations > 0) {
        timer.invocations--;
        if (timer.invocations === 0) this.unschedule(callback);
      }
      callback(elapsed);
    }
  }
}
