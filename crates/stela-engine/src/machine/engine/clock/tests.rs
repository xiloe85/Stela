use super::*;
use alloc::vec::Vec;

#[test]
fn repeated_slow_slices_charge_their_full_host_duration() {
    let mut clock = Clock::default();
    let mut previous = 0;
    // Four concurrent PEs get ten counts per forty blocks. Each slice
    // takes 100 host counts: after the initial overshoot, work must
    // advance the count by ten, not by a share of the host's slowdown.
    for slice in 0..100 {
        clock.start_slice(|| slice * 100, slice * 40, 4);
        let during = clock.read(|| slice * 100 + 50);
        assert!(during >= previous);
        clock.end_slice(|| (slice + 1) * 100, (slice + 1) * 40);
        let after = clock.read(|| (slice + 1) * 100);
        assert!(after >= during);
        assert_eq!(after, 100 + slice * 10);
        previous = after;
    }
    // The final read's bounded overshoot holds briefly, then ordinary
    // idle time reaches the translated deadline exactly.
    assert_eq!(clock.read(|| 10_089), 1090);
    assert_eq!(clock.read(|| 10_100), 1100);
    assert_eq!(clock.host_deadline(2000), 11_000);
    assert_eq!(clock.read(|| 10_999), 1999);
    assert_eq!(clock.read(|| 11_000), 2000);
}

#[test]
fn host_work_excluded_mid_slice_is_not_charged_again() {
    let mut clock = Clock::default();
    for slice in 0..100 {
        clock.start_slice(|| slice * 100, slice * 10, 1);
        // Eighty counts were host-only work; the other twenty ran ten
        // blocks. Only ten more counts become slice lag.
        clock.exclude(80);
        clock.end_slice(|| (slice + 1) * 100, (slice + 1) * 10);
        assert_eq!(clock.read(|| (slice + 1) * 100), 20 + slice * 10);
    }
    assert_eq!(clock.host_deadline(2000), 11_000);
    assert_eq!(clock.read(|| 11_000), 2000);
}

#[test]
fn idle_leaps_from_a_held_count_keep_deadlines_coherent() {
    let mut clock = Clock::default();
    clock.start_slice(|| 1000, 0, 1);
    clock.end_slice(|| 2000, 100);
    assert_eq!(clock.read(|| 2000), 2000);
    // The observed count is held above the host minus lag. Leaping to
    // 5000 must advance that underlying count too, so it can continue
    // from 5000 and translate the very next deadline correctly.
    clock.skip_to(2000, 5000);
    assert_eq!(clock.read(|| 2000), 5000);
    assert_eq!(clock.host_deadline(5001), 2001);
    assert_eq!(clock.read(|| 2001), 5001);
    clock.start_slice(|| 2001, 100, 1);
    clock.end_slice(|| 2101, 120);
    assert_eq!(clock.read(|| 2101), 5101);
    assert_eq!(clock.host_deadline(6000), 3080);
    assert_eq!(clock.read(|| 3079), 5999);
    assert_eq!(clock.read(|| 3080), 6000);
    clock.exclude(500);
    assert_eq!(clock.read(|| 3580), 6000);
}

#[test]
fn slices_advance_by_their_blocks_and_waits_by_the_host() {
    // The blocks that account for `counts` counts.
    let blocks = |counts: u64| counts / TICKS_PER_BLOCK;
    let mut clock = Clock::default();
    clock.start_slice(|| 1000, 0, 2);
    // Within a slice the host's counter shows through.
    assert_eq!(clock.read(|| 1500), 1500);
    // Blocks shared by two PEs account for 200 counts, so the 800 more
    // the slice showed become lag and the count holds.
    clock.end_slice(|| 2000, blocks(400));
    assert_eq!(clock.read(|| 2000), 2000);
    assert_eq!(clock.read(|| 2500), 2000);
    assert_eq!(clock.read(|| 2900), 2100);
    assert_eq!(clock.host_deadline(2200), 3000);
    // A slice within its bound adds no lag.
    clock.start_slice(|| 3000, blocks(400), 1);
    clock.end_slice(|| 3200, blocks(800));
    assert_eq!(clock.read(|| 3300), 2500);
    // Waiting outside a slice follows the host.
    assert_eq!(clock.read(|| 10_000), 9200);
    clock.end_slice(|| 11_000, blocks(800));
    assert_eq!(clock.read(|| 11_000), 10_200);
}

#[test]
fn excluded_host_time_never_reaches_the_count() {
    let mut clock = Clock::default();
    assert_eq!(clock.read(|| 100), 100);
    clock.exclude(900);
    assert_eq!(clock.read(|| 1000), 100);
    assert_eq!(clock.read(|| 1010), 110);
}

#[test]
fn idle_skips_leap_to_the_deadline_and_keep_counting_from_there() {
    let mut clock = Clock::default();
    assert_eq!(clock.read(|| 100), 100);
    clock.skip_to(150, 5_000);
    assert_eq!(clock.read(|| 150), 5_000);
    assert_eq!(clock.read(|| 200), 5_050);
    assert_eq!(clock.host_deadline(6_000), 1_150);
    // A deadline already reached leaps nowhere.
    clock.skip_to(300, 10);
    assert_eq!(clock.read(|| 300), 5_150);
    // Lag from a slow slice still holds the count back: 90 of the 100
    // counts it showed, until the host catches up.
    clock.start_slice(|| 300, 0, 1);
    clock.end_slice(|| 400, 10 / TICKS_PER_BLOCK);
    assert_eq!(clock.read(|| 490), 5_250);
    assert_eq!(clock.read(|| 500), 5_260);
}

/// A counter frequency at which the stall limit is 1200 counts and the
/// timer delay 240.
const FREQUENCY: u64 = 24_000;

/// Two PEs reading one shared clock, continuing a serialized count of
/// 1000 at host 1000.
fn pair() -> (Arc<SharedClock>, Clock, Clock) {
    let shared = Arc::new(SharedClock::new(2));
    let mut serialized = Clock::default();
    serialized.seed(&shared, 1000, FREQUENCY);
    let (mut first, mut second) = (Clock::default(), Clock::default());
    first.attach(Arc::clone(&shared), 0);
    second.attach(Arc::clone(&shared), 1);
    (shared, first, second)
}

#[test]
fn a_shared_count_is_the_host_s_however_slow_a_pe_runs() {
    let (shared, mut first, mut second) = pair();
    let sequence = shared.state.sequence.load(Ordering::Relaxed);
    first.start_slice(|| 1000, 0, 1);
    second.start_slice(|| 1000, 0, 1);
    // The first PE runs a million blocks a slice, the second one block in
    // all, and waits now and then: neither moves the count off the host's.
    for slice in 1..=100u64 {
        let host = 1000 + slice * 480;
        first.end_slice(|| host, slice * 1_000_000);
        first.start_slice(|| host, slice * 1_000_000, 1);
        if slice % 7 == 0 {
            second.wait(|| host);
        }
        assert_eq!(first.read(|| host), host);
        assert_eq!(second.read(|| host + 1), host + 1);
        assert_eq!(shared.count(|| host + 2), host + 2);
    }
    assert_eq!(second.host_deadline(90_000), 90_000);
    assert_eq!(second.wake_at(90_000, 50_000), 90_000);
    // Slices, waits and reads leave the state native reads share untouched.
    assert_eq!(shared.state.sequence.load(Ordering::Relaxed), sequence);
}

#[test]
fn readers_of_one_shared_count_never_disagree_backwards() {
    let (shared, mut first, mut second) = pair();
    let (mut shared_seen, mut seen) = (0, [0; 2]);
    for step in 0..400u64 {
        let host = 1000 + step * 37;
        if step % 50 == 0 {
            first.leave(|| host, 0, false);
        }
        if step % 50 == 45 {
            first.resume(|| host, 0);
        }
        let count = shared.count(|| host);
        assert!(
            count >= shared_seen,
            "step {step}: {count} after {shared_seen}"
        );
        shared_seen = count;
        // A reader whose host counter lags another core's still never
        // reads less than it read before.
        for (reader, clock) in [&mut first, &mut second].into_iter().enumerate() {
            let count = clock.read(|| host - (step + reader as u64) % 3 * 20);
            assert!(
                count >= seen[reader],
                "step {step}: {count} after {}",
                seen[reader]
            );
            seen[reader] = count;
        }
    }
}

#[test]
fn a_pe_stopped_in_the_kernel_holds_the_count_only_past_the_stall_limit() {
    let (shared, mut first, mut second) = pair();
    // A short stop in the kernel costs guest time nothing.
    assert!(first.leave(|| 2000, 0, false));
    assert!(!first.leave(|| 2000, 0, false), "stopped once");
    assert_eq!(second.read(|| 2500), 2500);
    first.resume(|| 2600, 0);
    first.mark(|| 2600, 1, None);
    assert_eq!(second.read(|| 3000), 3000);
    assert_eq!(shared.take_stalled(), 0);
    // A long one stops the count 1200 counts (50 ms) after its last
    // progress; the rest becomes lag, and the count goes on from there.
    first.mark(|| 4000, 2, None);
    assert!(first.leave(|| 4000, 0, false));
    assert_eq!(second.read(|| 6000), 5200);
    assert_eq!(
        second.wake_at(5300, 6000),
        7200,
        "looks again a limit later"
    );
    // Back in guest code it shows no progress until its next mark.
    first.resume(|| 10_000, 0);
    assert_eq!(second.read(|| 10_000), 5200);
    first.mark(|| 10_000, 3, None);
    assert_eq!(shared.take_stalled(), 4800);
    assert_eq!(second.read(|| 10_000), 5200);
    assert_eq!(second.read(|| 10_100), 5300);
    // As early as the slew can bring it: 10,800 less one count in 101.
    assert_eq!(second.host_deadline(6000), 10_793);
    // Host work at EL0 holds no lock the kernel times. Each look from here
    // repays a count of the lag for each 100 host counts.
    assert!(first.leave(|| 11_000, 0, true));
    second.look(|| 15_000);
    assert_eq!(second.read(|| 20_000), 15_250);
    first.resume(|| 20_000, 0);
    assert_eq!(shared.take_stalled(), 0);
    // Nor host work while its guest waits for an interrupt.
    first.idle(|| 20_000, 0, None);
    assert!(!first.leave(|| 20_000, 0, false));
    second.look(|| 30_000);
    assert_eq!(second.read(|| 30_000), 25_400);
    assert_eq!(shared.take_repaid(), 200);
}

/// 200 ms at the stall limit's 50 ms.
const LONG_STALL: u64 = 4 * FREQUENCY * STALL_MILLIS / 1000;

#[test]
fn a_pe_with_guest_work_that_retires_nothing_holds_the_count_only_past_the_limit() {
    let (shared, mut first, mut second) = pair();
    first.idle(|| 1000, 0, None);
    first.busy(|| 1000, 0, None);
    second.idle(|| 1000, 0, None);
    second.busy(|| 1000, 0, None);
    let limit = 1000 + FREQUENCY * STALL_MILLIS / 1000;
    // The first PE, in host work or a compile, retires nothing for 200 ms;
    // the second runs and marks its progress every 10 counts.
    let mut retired = 0;
    for host in (1010..1000 + LONG_STALL).step_by(10) {
        retired += 100;
        second.mark(|| host, retired, None);
        assert_eq!(second.read(|| host), host.min(limit), "at {host}");
    }
    assert_eq!(shared.take_stalled(), 0, "held, not yet lifted");
    // It runs again: the count goes on from the limit, and only the excess
    // over the limit became lag.
    first.mark(|| 1000 + LONG_STALL, 1, None);
    assert_eq!(shared.take_stalled(), LONG_STALL - (limit - 1000));
    assert_eq!(second.read(|| 1000 + LONG_STALL), limit);
    assert_eq!(second.read(|| 1100 + LONG_STALL), limit + 100);
    assert_eq!(shared.state.ceiling.load(Ordering::Relaxed), UNBOUNDED);
    // Its gap is reported, with no phase known without step timing; the
    // second PE's 10-count gaps are too short to name.
    assert_eq!(shared.take_stalls(), [(LONG_STALL, STALL_UNKNOWN), (0, 0)]);
    assert_eq!(shared.take_stalls(), [(0, 0), (0, 0)]);
}

#[test]
fn short_kernel_stops_with_no_progress_between_hold_the_count_as_one_long_stop() {
    let (shared, mut first, mut second) = pair();
    first.idle(|| 1000, 0, None);
    first.busy(|| 1000, 0, None);
    second.idle(|| 1000, 0, None);
    second.busy(|| 1000, 0, None);
    let limit = 1000 + FREQUENCY * STALL_MILLIS / 1000;
    // For 200 ms the first PE stops in the kernel for 4 ms of host work at
    // a time, as the boot PE does between slices, and runs a few blocks
    // between, never the instructions of a progress mark; the second runs.
    let mut retired = 0;
    for host in (1000..1000 + LONG_STALL).step_by(120) {
        assert!(first.leave(|| host, 0, false));
        first.resume(|| host + 100, 0);
        retired += 100;
        second.mark(|| host + 110, retired, None);
        assert_eq!(
            second.read(|| host + 110),
            (host + 110).min(limit),
            "at {host}"
        );
    }
    // Its next mark lifts the hold; only the excess became lag.
    let end = 1000 + LONG_STALL;
    first.mark(|| end, 1, None);
    assert_eq!(shared.take_stalled(), end - limit);
    assert_eq!(second.read(|| end + 100), limit + 100);
    // Stops between progress marks cost the count nothing.
    for host in (end..end + LONG_STALL).step_by(120) {
        assert!(first.leave(|| host, 0, false));
        first.resume(|| host + 100, 0);
        first.mark(|| host + 110, host, None);
        second.mark(|| host + 110, host, None);
    }
    assert_eq!(shared.take_stalled(), 0);
    // Nor do stops at EL0, which hold nothing.
    let later = end + LONG_STALL;
    for host in (later..later + LONG_STALL).step_by(120) {
        assert!(first.leave(|| host, 0, true));
        first.resume(|| host + 100, 0);
        second.mark(|| host + 110, host, None);
    }
    assert_eq!(shared.take_stalled(), 0);
}

#[test]
fn a_waiting_pe_sets_the_limit_of_a_stalled_one() {
    let (shared, mut first, mut second) = pair();
    first.idle(|| 1000, 0, None);
    first.busy(|| 1000, 0, None);
    second.idle(|| 1000, 0, None);
    // The only other PE waits, and looks when it wakes.
    second.look(|| 1100);
    assert_eq!(shared.state.ceiling.load(Ordering::Relaxed), UNBOUNDED);
    second.look(|| 1400);
    assert_eq!(shared.count(|| 5000), 2200);
    first.mark(|| 5000, 1, None);
    assert_eq!(shared.take_stalled(), 2800);
}

/// A WFE that spins on its Exclusives monitor is a ticket waiter, the next
/// owner once the holder hands it the turn: kept from its passes past the
/// stall limit, it holds the count as a stopped holder does. Its passes
/// retire nothing yet are its progress, so while it spins nothing holds.
/// At EL0, where no lease starts, the stall limit alone applies; a kernel
/// spin this long runs on a lease (`a_long_host_step_under_an_old_spin_*`).
#[test]
fn a_spinning_wfe_the_host_keeps_from_its_passes_holds_the_count() {
    let (shared, mut first, mut second) = pair();
    // It ran, then waited; its WFE now spins, though it retires nothing.
    first.idle(|| 1000, 7, None);
    assert!(!first.has_guest_work());
    let limit_after = |host: u64| host + FREQUENCY * STALL_MILLIS / 1000;
    // A pass every 100 counts publishes each time.
    let mut retired = 0;
    for host in (1000..1000 + LONG_STALL).step_by(100) {
        first.spin(|| host, 7, None, true);
        retired += 100;
        second.mark(|| host, retired, None);
        assert_eq!(
            second.read(|| host),
            host,
            "spinning holds nothing at {host}"
        );
    }
    assert!(first.has_guest_work());
    assert_eq!(shared.take_stalled(), 0);
    // The host keeps it from its passes for 200 ms: the count holds a stall
    // limit after its last pass.
    let last = 1000 + LONG_STALL - 100;
    let end = last + LONG_STALL;
    for host in (last + 100..end).step_by(10) {
        retired += 100;
        second.mark(|| host, retired, None);
        assert_eq!(
            second.read(|| host),
            host.min(limit_after(last)),
            "at {host}"
        );
    }
    // Its next pass lifts the hold; only the excess became lag.
    first.spin(|| end, 7, None, true);
    assert_eq!(shared.take_stalled(), end - limit_after(last));
    assert_eq!(second.read(|| end + 100), limit_after(last) + 100);
    // Waiting without the spin, it holds nothing again.
    first.idle(|| end + 100, 7, None);
    assert!(!first.has_guest_work());
    for host in (end + 100..end + 100 + LONG_STALL).step_by(10) {
        retired += 100;
        second.mark(|| host, retired, None);
    }
    assert_eq!(shared.take_stalled(), 0);
    assert_eq!(shared.state.ceiling.load(Ordering::Relaxed), UNBOUNDED);
}

#[test]
fn waiting_pes_hold_nothing() {
    let (shared, mut first, mut second) = pair();
    let sequence = shared.state.sequence.load(Ordering::Relaxed);
    first.idle(|| 1000, 0, None);
    first.busy(|| 1000, 0, None);
    first.idle(|| 1010, 5, None);
    // Its guest waits 200 ms; the other runs and looks.
    let mut retired = 0;
    for host in (1000..1000 + LONG_STALL).step_by(10) {
        retired += 100;
        second.mark(|| host, retired, None);
        assert_eq!(second.read(|| host), host);
    }
    assert_eq!(shared.take_stalled(), 0);
    assert_eq!(shared.state.sequence.load(Ordering::Relaxed), sequence);
}

#[test]
fn running_pes_keep_the_count_at_the_host_s_rate() {
    let (shared, mut first, mut second) = pair();
    let sequence = shared.state.sequence.load(Ordering::Relaxed);
    let start = 1000;
    first.idle(|| start, 0, None);
    first.busy(|| start, 0, None);
    second.idle(|| start, 0, None);
    second.busy(|| start, 0, None);
    // Ten seconds of both running, one slower and in uneven steps.
    let end = start + 10 * FREQUENCY;
    let mut retired = [0; 2];
    for (step, host) in (start..end).step_by(7).enumerate() {
        retired[0] += 3;
        first.mark(|| host, retired[0], None);
        if step % 3 == 0 {
            retired[1] += 1;
            second.mark(|| host, retired[1], None);
        }
    }
    assert_eq!(second.read(|| end), end);
    assert_eq!(shared.take_stalled(), 0);
    // Not a single write to the state native reads share.
    assert_eq!(shared.state.sequence.load(Ordering::Relaxed), sequence);
}

#[test]
fn a_gap_is_named_by_the_phase_that_took_most_of_it() {
    let then = Some((0, [0; super::super::step_timing::Phase::COUNT]));
    // Fetch (compiling) took 60 of 100 ticks.
    assert_eq!(Clock::stall_phase(then, Some((100, [10, 0, 60, 5, 5]))), 2);
    // Host work outside the step loop took 95.
    assert_eq!(
        Clock::stall_phase(then, Some((100, [1, 1, 1, 1, 1]))),
        STALL_HOST
    );
    assert_eq!(Clock::stall_phase(None, then), STALL_UNKNOWN);
}

#[test]
fn two_stops_hold_the_count_at_the_earlier_one_s_limit() {
    let (shared, mut first, mut second) = pair();
    assert!(first.leave(|| 2000, 0, false));
    assert!(second.leave(|| 2500, 0, false));
    assert_eq!(shared.count(|| 9000), 3200);
    first.resume(|| 9000, 0);
    first.mark(|| 9000, 1, None);
    // The second stop's limit, 3700, now holds the count from 3200.
    assert_eq!(shared.count(|| 9400), 3600);
    assert_eq!(shared.count(|| 9600), 3700);
    second.resume(|| 9600, 0);
    second.mark(|| 9600, 1, None);
    assert_eq!(shared.count(|| 9700), 3800);
    assert_eq!(shared.take_stalled(), 5800 + 100);
}

#[test]
fn a_busy_pe_s_timers_wait_for_its_share_of_work() {
    let (_, mut first, _) = pair();
    // Its first timer interrupt waits for nothing.
    assert_eq!(first.timer_due(Some(900)), Some(900));
    first.timer_taken(1000, 0);
    assert_eq!(first.timer_due(Some(900)), Some(1000));
    // It then retired half the work in 100 counts: its timers wait 200
    // counts from the next interrupt.
    first.timer_taken(1100, TIMER_WORK / 2);
    assert_eq!(first.timer_due(Some(1150)), Some(1300));
    assert_eq!(first.timer_due(Some(1400)), Some(1400));
    // A PE that waits had time to spare: its timers signal when due.
    first.wait(|| 1200);
    assert_eq!(first.timer_due(Some(1250)), Some(1250));
    first.timer_taken(1250, TIMER_WORK);
    assert_eq!(first.timer_due(Some(1250)), Some(1250));
    // A serialized clock's timers are never held.
    assert_eq!(Clock::default().timer_due(Some(5)), Some(5));
}

#[test]
fn a_pe_back_from_a_long_host_pause_takes_no_flood_of_timer_interrupts() {
    let (_, mut first, _) = pair();
    // The guest rearms its timer in the past at each interrupt, as a
    // periodic timer that fell behind does. The PE retires TIMER_WORK in
    // 100 counts, except during a host pause of 5000 counts.
    let (mut retired, mut deliveries) = (0u64, Vec::new());
    for count in 1000..20_000u64 {
        let paused = (5000..10_000).contains(&count);
        if !paused {
            retired += TIMER_WORK / 100;
        }
        if !paused && first.timer_due(Some(0)).is_some_and(|due| count >= due) {
            first.timer_taken(count, retired);
            deliveries.push(count);
        }
    }
    // After the first two, interrupts come 100 counts apart while it runs,
    // and the first after the pause waits out the 240-count delay at most.
    for pair in deliveries.windows(2).skip(1) {
        let gap = pair[1] - pair[0];
        assert!(gap >= 100 || pair[0] < 1002, "{pair:?}");
    }
    let after = deliveries
        .iter()
        .position(|&count| count >= 10_000)
        .unwrap();
    assert_eq!(deliveries[after], 10_000);
    assert_eq!(deliveries[after + 1] - deliveries[after], 240);
    assert!(deliveries.len() < (20_000 - 1000 - 5000) / 100 + 4);
}

/// `pes` PEs reading one shared clock seeded at count 1000 at host 1000,
/// counting at `frequency` Hz, each with guest work from host 1000.
fn busy_clocks(pes: usize, frequency: u64) -> (Arc<SharedClock>, Vec<Clock>) {
    let shared = Arc::new(SharedClock::new(pes));
    Clock::default().seed(&shared, 1000, frequency);
    let clocks = (0..pes)
        .map(|pe| {
            let mut clock = Clock::default();
            clock.attach(Arc::clone(&shared), pe);
            clock.idle(|| 1000, 0, None);
            clock.busy(|| 1000, 0, None);
            clock
        })
        .collect();
    (shared, clocks)
}

#[test]
fn a_stall_s_lag_is_repaid_by_a_bounded_slew_with_timers_on_their_count() {
    let (shared, mut clocks) = busy_clocks(3, FREQUENCY);
    let stall = FREQUENCY * STALL_MILLIS / 1000;
    // PE 2 waits, for a timer re-armed 1000 counts after each one fires.
    clocks[2].idle(|| 1000, 0, None);
    let step = shared.slew_step();
    let (mut deadline, mut fired, mut armed) = (1500, 0, 1000);
    let mut wake = clocks[2].wake_at(deadline, 1000);
    // PE 0 retires nothing for 200 ms, then runs; PE 1 runs throughout.
    let resumed = 1000 + LONG_STALL;
    let lag = LONG_STALL - stall;
    let repaid_by = resumed + lag * SLEW_PARTS + 2 * step;
    let end = repaid_by + 10_000;
    let (mut retired, mut seen, mut repaid) = ([0u64; 2], [0u64; 3], 0);
    for host in 1001..end {
        if host % 10 == 0 {
            for pe in 0..2 {
                if pe == 0 && host < resumed {
                    continue;
                }
                retired[pe] += 100;
                clocks[pe].mark(|| host, retired[pe], None);
            }
            // A PE that is sure the deadline is ahead is right.
            if clocks[1].below(deadline, || host) {
                assert!(shared.count(|| host) < deadline, "at {host}");
            }
        }
        if host == wake {
            clocks[2].look(|| host);
            let count = clocks[2].read(|| host);
            if count >= deadline {
                // On its count, a count or two late at most for the steps'
                // rounding; one armed before the hold was lifted waited for
                // the stopped PE.
                if armed > resumed {
                    assert!(count - deadline <= 2, "at {host}: {count} for {deadline}");
                }
                fired += 1;
                deadline = count + 1000;
                armed = host;
            }
            wake = clocks[2].wake_at(deadline, host);
            assert!(wake > host, "at {host}: waits again at once");
        }
        // Monotonic for every reader; never past the host's count.
        for (pe, clock) in clocks.iter_mut().enumerate() {
            let count = clock.read(|| host);
            assert!(count >= seen[pe], "at {host}: {count} after {}", seen[pe]);
            assert!(count <= host, "at {host}: overshoot to {count}");
            seen[pe] = count;
        }
        if host == resumed + 1 {
            // Held 50 ms in, the rest became lag.
            assert_eq!(shared.lag(), lag);
            assert_eq!(seen[1], 1000 + stall + 1);
            assert_eq!(shared.take_repaid(), 0);
        }
        if host > resumed && host < repaid_by - 2 * step {
            // No slew while held; after it, about one count per hundred.
            let since = host - resumed;
            let owed = shared.lag();
            assert!(owed > 0, "at {host}");
            let expected = lag - since / SLEW_PARTS;
            assert!(
                owed.abs_diff(expected) <= step / SLEW_PARTS + 2,
                "at {host}: {owed}"
            );
        }
        if host % 1000 == 0 {
            repaid += shared.take_repaid();
        }
    }
    repaid += shared.take_repaid();
    assert_eq!((shared.lag(), repaid), (0, lag));
    assert_eq!(clocks[1].read(|| end), end, "back on the host's count");
    assert!(fired > lag * SLEW_PARTS / 1100, "{fired} timers fired");
}

#[test]
fn no_slew_runs_while_a_hold_is_in_force() {
    let (shared, mut first, second) = pair();
    // A first stop leaves lag, a second one holds the count again.
    assert!(first.leave(|| 2000, 0, false));
    second.look(|| 8000);
    first.resume(|| 8000, 0);
    first.mark(|| 8000, 1, None);
    let lag = shared.lag();
    assert_eq!(lag, 8000 - 3200);
    assert!(first.leave(|| 9000, 0, false));
    for host in (9000..40_000).step_by(50) {
        second.look(|| host);
    }
    assert_eq!(shared.take_repaid(), 0);
    first.resume(|| 40_000, 0);
    first.mark(|| 40_000, 2, None);
    assert!(shared.lag() > lag);
}

#[test]
fn a_look_reads_no_slot_until_one_could_look_stalled() {
    // 256 PEs at 24 MHz, each marking every 10,000 host counts in turn.
    const PES: usize = 256;
    let frequency = 24_000_000;
    let (shared, mut clocks) = busy_clocks(PES, frequency);
    shared.loads.store(0, Ordering::Relaxed);
    let span = 10 * frequency;
    let mut retired = [0u64; PES];
    let mut looks = 0u64;
    for (turn, host) in (1000..1000 + span).step_by(10_000 / PES).enumerate() {
        let pe = turn % PES;
        let published = clocks[pe].shared.as_ref().unwrap().progress.published;
        retired[pe] += 1000;
        clocks[pe].mark(|| host, retired[pe], None);
        looks += u64::from(clocks[pe].shared.as_ref().unwrap().progress.published != published);
    }
    // A scan of every slot once a part of the limit (12.5 ms) at most.
    let suspect = frequency * STALL_MILLIS / 1000 / SUSPECT_PARTS;
    let scans = span / (suspect / 2) + 2;
    let loads = shared.loads.load(Ordering::Relaxed);
    assert!(looks > 30_000, "{looks} looks");
    assert!(
        loads <= scans * PES as u64,
        "{loads} slot loads for {looks} looks"
    );
    assert_eq!(shared.count(|| 1000 + span), 1000 + span);
    assert_eq!(shared.take_stalled(), 0);
}

/// The guest time a ticket waiter, PE 0, sees from the start of its spin
/// to its turn, behind PEs 1 to `ahead` in that order. At its turn each is
/// kept from running for `stall` host counts by the host, or by a host
/// step of ours in the kernel when `steps`, then runs a short section and
/// hands the lock on; until then it spins too. Also the count at each
/// hand-off.
fn ticket_queue(ahead: usize, stall: u64, steps: bool) -> (u64, Vec<u64>) {
    const SECTION: u64 = 5;
    let (shared, mut clocks) = busy_clocks(ahead + 1, FREQUENCY);
    let mut retired = alloc::vec![0u64; ahead + 1];
    let mut host = 1000;
    let mut handoffs = Vec::new();
    for turn in 1..=ahead {
        let start = host;
        if steps {
            assert!(clocks[turn].leave(|| host, 0, false));
        }
        while host < start + stall + SECTION {
            let kept = host < start + stall;
            if steps && host == start + stall {
                clocks[turn].resume(|| host, 0);
            }
            for pe in (0..=ahead).filter(|&pe| pe == 0 || pe >= turn) {
                if pe == turn && kept {
                    continue;
                }
                if pe == turn {
                    retired[pe] += 10;
                    clocks[pe].mark(|| host, retired[pe], None);
                } else {
                    clocks[pe].spin(|| host, retired[pe], None, false);
                }
            }
            host += 1;
        }
        // It releases the lock, and its guest waits.
        clocks[turn].idle(|| host, retired[turn], None);
        handoffs.push(clocks[0].read(|| host));
    }
    assert_eq!(clocks[0].spin_started(), Some(1000));
    drop(shared);
    (handoffs[ahead - 1] - 1000, handoffs)
}

/// A waiter's budget behind `ahead` PEs that each stall once: the spin
/// age at which the lease starts, then a lease, a mark and the section
/// for each.
fn queue_budget(ahead: usize) -> u64 {
    let lease = FREQUENCY * LEASE_MILLIS / 1000;
    FREQUENCY * SPIN_MILLIS / 1000 + ahead as u64 * (lease + 2 + 5)
}

/// XNU's ticket spinlock timeout, 3,000,000 counts at 24 MHz.
const TICKET_TIMEOUT: u64 = FREQUENCY * 125 / 1000;

/// Each stall stays under the stall limit, so none holds the count on its
/// own; added up along one queue they passed the ticket timeout. Once the
/// waiter's spin is old, each costs it at most a lease.
#[test]
fn stalls_under_the_limit_along_one_ticket_queue_stay_within_the_spin_budget() {
    let stall = FREQUENCY * (STALL_MILLIS - 1) / 1000;
    for ahead in [2, 3] {
        let (waited, handoffs) = ticket_queue(ahead, stall, false);
        assert!(
            waited <= queue_budget(ahead),
            "{ahead} ahead: waited {waited} counts, budget {}",
            queue_budget(ahead)
        );
        assert!(waited < TICKET_TIMEOUT / 2, "{ahead} ahead: {waited}");
        // Every hand-off once the spin is old takes a lease and a section.
        let lease = FREQUENCY * LEASE_MILLIS / 1000;
        for pair in handoffs.windows(2) {
            assert!(pair[1] - pair[0] <= lease + 2 + 5, "{handoffs:?}");
        }
    }
}

/// Our own host steps in the kernel are stalls like any other: three PEs
/// each stopped just under the limit while holding the lock in turn.
#[test]
fn long_host_steps_along_one_ticket_queue_stay_within_the_spin_budget() {
    let stall = FREQUENCY * (STALL_MILLIS - 1) / 1000;
    let (waited, handoffs) = ticket_queue(3, stall, true);
    assert!(waited <= queue_budget(3), "waited {waited}: {handoffs:?}");
    // Without an old spin the same steps cost the count nothing.
    let (shared, mut clocks) = busy_clocks(2, FREQUENCY);
    let mut host = 1000;
    for step in 0..3u64 {
        assert!(clocks[1].leave(|| host, 0, false));
        for _ in 0..stall {
            clocks[0].mark(|| host, host, None);
            assert_eq!(clocks[0].read(|| host), host);
            host += 1;
        }
        clocks[1].resume(|| host, 0);
        clocks[1].mark(|| host, step + 1, None);
    }
    assert_eq!(shared.take_stalled(), 0);
}

/// A host step of ours in the kernel, while a spin is old, holds the count
/// a lease after its PE's last progress; the held time is reported as the
/// lease's and repaid as lag.
#[test]
fn a_long_host_step_under_an_old_spin_holds_the_count_a_lease_after_its_progress() {
    let (shared, mut clocks) = busy_clocks(3, FREQUENCY);
    let (spin, lease) = (
        FREQUENCY * SPIN_MILLIS / 1000,
        FREQUENCY * LEASE_MILLIS / 1000,
    );
    let mut host = 1000;
    // PE 0 spins on a lock PE 1 holds and runs; PE 2 runs.
    while host < 1000 + spin + 100 {
        clocks[0].spin(|| host, 0, None, false);
        clocks[1].mark(|| host, host, None);
        clocks[2].mark(|| host, host, None);
        assert_eq!(
            clocks[0].read(|| host),
            host,
            "a running queue holds nothing"
        );
        host += 1;
    }
    // PE 2 stops in the kernel for 10 ms of host work.
    let step = FREQUENCY * 10 / 1000;
    let last = host - 1;
    assert!(clocks[2].leave(|| host, 0, false));
    for _ in 0..step {
        clocks[0].spin(|| host, 0, None, false);
        clocks[1].mark(|| host, host, None);
        assert!(clocks[0].read(|| host) <= last + lease + 1, "at {host}");
        host += 1;
    }
    clocks[2].resume(|| host, 0);
    clocks[2].mark(|| host, host, None);
    let stalled = shared.take_stalled();
    assert!(stalled + lease + 2 >= step, "{stalled} held");
    assert_eq!(shared.take_leased(), stalled);
    assert_eq!(clocks[1].read(|| host + 1), host + 1 - stalled);
}

/// Every PE kept from running at once, as by a firmware SMI or a paused
/// VM: no PE is left to see the stall, and the lease set ahead holds.
#[test]
fn every_pe_stopped_at_once_under_an_old_spin_costs_at_most_a_lease() {
    let (shared, mut clocks) = busy_clocks(2, FREQUENCY);
    let (spin, lease) = (
        FREQUENCY * SPIN_MILLIS / 1000,
        FREQUENCY * LEASE_MILLIS / 1000,
    );
    let mut host = 1000;
    while host < 1000 + spin + 100 {
        clocks[0].spin(|| host, 0, None, false);
        clocks[1].mark(|| host, host, None);
        host += 1;
    }
    let last = host - 1;
    host += FREQUENCY / 10;
    assert!(clocks[0].read(|| host) <= last + lease, "100 ms later");
    assert!(shared.count(|| host) <= last + lease);
}

/// On a healthy host the count is the host's at every read: short spins
/// write nothing, and an old spin with every PE marking well within a
/// lease holds nothing either.
#[test]
fn a_healthy_run_s_count_is_the_host_s_with_spins_short_and_long() {
    let (shared, mut clocks) = busy_clocks(3, FREQUENCY);
    let sequence = shared.state.sequence.load(Ordering::Relaxed);
    let mut retired = [0u64; 3];
    let mut host = 1000;
    // Ten seconds of 2 ms kernel spins on PE 0, one every 5 ms, between
    // which it runs 360,000 instructions or waits, in turn.
    let end = 1000 + 10 * FREQUENCY;
    while host < end {
        let (round, phase) = ((host - 1000) / 120, (host - 1000) % 120);
        if phase < 48 {
            clocks[0].spin(|| host, retired[0], None, false);
        } else if round % 2 == 0 && phase % 2 == 0 {
            retired[0] += 10_000;
            clocks[0].mark(|| host, retired[0], None);
        } else if round % 2 == 1 && phase == 48 {
            clocks[0].idle(|| host, retired[0], None);
        }
        for pe in 1..3 {
            retired[pe] += 100;
            clocks[pe].mark(|| host, retired[pe], None);
        }
        assert_eq!(clocks[1].read(|| host), host, "at {host}");
        host += 1;
    }
    assert_eq!(shared.state.sequence.load(Ordering::Relaxed), sequence);
    // A 60 ms spin behind a holder that runs, PE 2 marking only every
    // 1.5 ms: past the spin age the count runs on its lease, never held.
    let gap = FREQUENCY * 3 / 2000;
    let start = host;
    while host < start + FREQUENCY * 60 / 1000 {
        clocks[0].spin(|| host, retired[0], None, false);
        retired[1] += 100;
        clocks[1].mark(|| host, retired[1], None);
        if (host - start) % gap == 0 {
            retired[2] += 100;
            clocks[2].mark(|| host, retired[2], None);
        }
        assert_eq!(clocks[1].read(|| host), host, "at {host}");
        host += 1;
    }
    assert!(
        shared.state.ceiling.load(Ordering::Relaxed) != UNBOUNDED,
        "on a lease"
    );
    assert_eq!(shared.take_stalled(), 0);
    // It takes the lock and runs on: the spin ends, the lease with it.
    for _ in 0..FREQUENCY / 10 {
        retired[0] += SPIN_END_INSTRUCTIONS / 100;
        for pe in 0..3 {
            retired[pe] += 100;
            clocks[pe].mark(|| host, retired[pe], None);
        }
        assert_eq!(clocks[1].read(|| host), host);
        host += 1;
    }
    assert_eq!(clocks[0].spin_started(), None);
    assert_eq!(shared.state.ceiling.load(Ordering::Relaxed), UNBOUNDED);
    assert_eq!(shared.take_stalled(), 0);
}

/// A spin at EL0 is timed by nothing in the kernel: it keeps guest work
/// but starts no lease.
#[test]
fn a_spin_at_el0_starts_no_lease() {
    let (shared, mut clocks) = busy_clocks(2, FREQUENCY);
    for host in 1000..1000 + 4 * FREQUENCY * SPIN_MILLIS / 1000 {
        clocks[0].spin(|| host, 0, None, true);
        clocks[1].mark(|| host, host, None);
    }
    assert!(clocks[0].has_guest_work());
    assert_eq!(clocks[0].spin_started(), None);
    assert_eq!(shared.state.ceiling.load(Ordering::Relaxed), UNBOUNDED);
}
