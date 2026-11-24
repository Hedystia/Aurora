/** Elapsed simulation seconds since the previous invocation or scheduling. */
export type ScheduledCallback = (elapsed: number) => void;
/** Timers owned by one component and advanced at a simulation phase boundary. */
export declare class Scheduler {
    private timers;
    schedule(callback: ScheduledCallback, interval: number, repeat: number, delay: number): void;
    unschedule(callback: ScheduledCallback): void;
    clear(): void;
    tick(dt: number): void;
}
