//! A host replay of the ring under a launch-like working set: the real
//! [`Ring`] and ghosts ([`super::super::churn::Churn`]) take installs from a
//! model of a guest whose active code exceeds one turn of the ring while an
//! app launches, as `PES CODE` shows in VM runs (parts retired every 20-30
//! s, two thirds of installs re-installs within one turn).
//!
//! The guest: kernel code that exits to the engine at every block (each
//! entry a fetch, no native transfer), daemons' and apps' code that runs
//! chained (native transfers; the engine sees it once a run's budget, one
//! fetch in [`CHAIN_LIMIT`] transfers), and four apps launched in turn,
//! twice: each with code it keeps running and code it runs only while it
//! starts. A part's heat is what the engine counts of it: fetches alone
//! ([`Heat::Fetches`], as before), or fetches and sampled native transfers
//! ([`Heat::Sampled`], `Machine::heat`).
use super::super::churn::{self, Churn, ChurnCounts, Gone};
use super::{Advance, Grace, Ring, TENURED, YOUNG};
use alloc::{collections::VecDeque, vec, vec::Vec};

/// One block's code: every block takes this much of its part.
const BLOCK_BYTES: usize = 64;
const PARTS: usize = 32;
const BLOCKS_PER_PART: usize = 256;
/// What a run's budget allows before it returns to the engine.
const CHAIN_LIMIT: u64 = 4096;
/// A sample's odds and weight, as `Machine::sampled_budget` draws them
/// without step timing: one run in 128, a transfer of up to 4096.
const SAMPLE_WEIGHT: u64 = 128 * 4096;
/// Fetches an install makes: its own and its predecessors' until linked.
const INSTALL_FETCHES: u64 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Heat {
    Fetches,
    Sampled,
}

/// How a replay's ring works: its heat, whether a late thunk or tail of a
/// block of a full part goes in that part's late room
/// ([`Ring::take_late`]) or in its generation's open part, which it pins
/// ([`Ring::refer`]), as before; and the blocks whose stored form it keeps
/// once compiled again.
#[derive(Clone, Copy, Debug)]
pub(super) struct Setup {
    pub(super) heat: Heat,
    pub(super) late: bool,
    pub(super) kept: usize,
}

/// The odds that an entered block of a full part makes a thunk or tail in
/// a tick: a link made late, a hot exit's stamped tail.
const LATE_LINK: f64 = 0.002;
const THUNK_BYTES: usize = 64;

/// Chained code makes about 2000 times the transfers per tick that exiting
/// code makes fetches, so that a ring choice sees tens of samples a part,
/// as the VM's PEs give (about 200 samples a second each, a choice every
/// second or so, 32 parts).
///
/// What a group of blocks does in a tick it is active in: each block
/// entered with odds falling exponentially with its rank (from 1 to
/// `last`, the odds of its last block), and per entry `fetches` engine
/// fetches (code that exits at every block) or `transfers` native
/// transfers.
#[derive(Clone, Copy)]
struct Group {
    first: usize,
    blocks: usize,
    last: f64,
    fetches: u64,
    transfers: u64,
}

impl Group {
    fn odds(&self, rank: usize) -> f64 {
        (self.last.ln() * rank as f64 / self.blocks as f64).exp()
    }
}

/// What a replay counted after its warm-up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Outcome {
    pub(super) installs: u64,
    /// Installs of a block the ring took away before, and of those, within
    /// its tenure age (promoted to the tenured generation): code the guest
    /// kept running.
    pub(super) again: u64,
    pub(super) promoted: u64,
    /// Installs that compiled: the rest came from kept bytes.
    pub(super) compiles: u64,
    pub(super) kept_hits: u64,
    pub(super) retired: u64,
    /// Cold choices a reference kept from the tenured part entered least.
    pub(super) pinned: u64,
}

/// Blocks a replay keeps the stored form of after compiling them again
/// within the ring's tenure age, least recently used first out
/// (`rebase::Rebases` with re-install keeping).
struct Kept {
    capacity: usize,
    stamps: Vec<u64>,
    order: VecDeque<(usize, u64)>,
    held: usize,
    stamp: u64,
}

impl Kept {
    fn new(blocks: usize, capacity: usize) -> Self {
        Self {
            capacity,
            stamps: vec![0; blocks],
            order: VecDeque::new(),
            held: 0,
            stamp: 0,
        }
    }

    fn has(&self, block: usize) -> bool {
        self.stamps[block] != 0
    }

    fn touch(&mut self, block: usize) {
        if self.capacity == 0 {
            return;
        }
        if self.stamps[block] == 0 {
            while self.held >= self.capacity {
                let (oldest, stamp) = self.order.pop_front().expect("a kept block");
                if self.stamps[oldest] == stamp {
                    self.stamps[oldest] = 0;
                    self.held -= 1;
                }
            }
            self.held += 1;
        }
        self.stamp += 1;
        self.stamps[block] = self.stamp;
        self.order.push_back((block, self.stamp));
    }
}

struct Model {
    ring: Ring,
    grace: Grace,
    churn: Churn,
    counts: ChurnCounts,
    /// Per block: the part it is installed in, if it is.
    part_of: Vec<Option<u32>>,
    /// Per part: its blocks.
    blocks_of: Vec<Vec<usize>>,
    kept: Kept,
    counting: bool,
    outcome: Outcome,
}

impl Model {
    fn identity(block: usize) -> u64 {
        churn::identity(
            0x1_0000_0000 + 4 * block as u64,
            0x8000_0000 + 4 * block as u64,
            0,
        )
    }

    fn retire(&mut self, part: u32) {
        let generation = self.ring.generation();
        for block in core::mem::take(&mut self.blocks_of[part as usize]) {
            self.part_of[block] = None;
            self.churn.gone(
                &mut self.counts,
                Self::identity(block),
                0,
                generation,
                Gone::Ring,
            );
        }
        self.ring.retired(part, self.grace.snapshot());
        if self.counting {
            self.outcome.retired += 1;
        }
    }

    /// A block of `part` makes a thunk or tail: in the part's late room,
    /// or the open part of its generation, which it pins.
    fn late_link(&mut self, part: usize, late: bool) {
        if late && let Some(place) = self.ring.take_late(part, THUNK_BYTES, 16) {
            self.ring.release(place);
            return;
        }
        let stream = self.ring.kind(part);
        // A link's thunk takes no new part: it waits for the next.
        if let Ok(place) = self.ring.take(stream, THUNK_BYTES, 16) {
            self.ring.refer(part, place.part as usize);
            self.ring.release(place);
        }
    }

    /// Install `block`, from its kept form when there is one: the part it
    /// went to.
    fn install(&mut self, block: usize) -> u32 {
        let identity = Self::identity(block);
        let generation = self.ring.generation();
        let promoted = self.churn.promotes(identity, generation, PARTS as u32);
        let stream = if promoted { TENURED } else { YOUNG };
        let from_kept = self.kept.has(block);
        if from_kept || promoted {
            self.kept.touch(block);
        }
        let again = self.counts.again;
        let place = loop {
            match self.ring.take(stream, BLOCK_BYTES, 16) {
                Ok(place) => break place,
                Err(full) => match self.ring.advance(stream, full, &self.grace, None) {
                    Advance::Moved => {}
                    Advance::Retire(part, _) => self.retire(part),
                    other => panic!("a ring nobody holds: {other:?}"),
                },
            }
        };
        self.ring.release(place);
        self.churn.installed(
            &mut self.counts,
            identity,
            0,
            self.ring.generation(),
            None,
            PARTS as u32,
        );
        self.ring.installed(place.part as usize);
        self.part_of[block] = Some(place.part);
        self.blocks_of[place.part as usize].push(block);
        if self.counting {
            self.outcome.installs += 1;
            self.outcome.again += u64::from(self.counts.again != again);
            self.outcome.promoted += u64::from(promoted);
            if from_kept {
                self.outcome.kept_hits += 1;
            } else {
                self.outcome.compiles += 1;
            }
        }
        place.part
    }
}

/// A small pseudo-random step.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn uniform(seed: &mut u64) -> f64 {
    (next(seed) >> 11) as f64 / (1u64 << 53) as f64
}

/// Replay `ticks` ticks of four apps launched in turn, twice, with `heat`,
/// keeping the stored form of up to `kept` blocks compiled again; what the
/// second half counted.
pub(super) fn replay(setup: Setup, ticks: u64) -> Outcome {
    let Setup { heat, late, kept } = setup;
    // Code the guest keeps running: under half the ring, which the
    // tenured generation (three quarters of it) can hold.
    let kernel = Group {
        first: 0,
        blocks: 600,
        last: 0.2,
        fetches: 50,
        transfers: 0,
    };
    let daemons = Group {
        first: 600,
        blocks: 1000,
        last: 0.05,
        fetches: 0,
        transfers: 100_000,
    };
    let app = |n: usize| Group {
        first: 1600 + n * 4000,
        blocks: 2000,
        last: 0.1,
        fetches: 0,
        transfers: 100_000,
    };
    // Code an app runs while it starts, and code run once (a pool the
    // replay goes through, 40 blocks a tick): what young parts take.
    let start = |n: usize| Group {
        first: 1600 + n * 4000 + 2000,
        blocks: 2000,
        last: 0.2,
        fetches: 0,
        transfers: 20_000,
    };
    const POOL: usize = 20_000;
    const ONCE_PER_TICK: usize = 40;
    let pool = 1600 + 4 * 4000;
    let blocks = pool + POOL;
    let mut model = Model {
        ring: Ring::new(PARTS, BLOCKS_PER_PART * BLOCK_BYTES),
        grace: Grace::new(1),
        churn: Churn::new(1 << 16),
        counts: ChurnCounts::default(),
        part_of: vec![None; blocks],
        blocks_of: vec![Vec::new(); PARTS],
        kept: Kept::new(blocks, kept),
        counting: false,
        outcome: Outcome::default(),
    };
    let phase = ticks / 8;
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut heat_of = vec![0f64; PARTS];
    // Each block's odds of entry in a tick its group is active in.
    let mut odds = vec![0f64; blocks];
    for n in 0..4 {
        for group in [kernel, daemons, app(n), start(n)] {
            for rank in 0..group.blocks {
                odds[group.first + rank] = group.odds(rank);
            }
        }
    }
    for tick in 0..ticks {
        model.counting = tick >= ticks / 2;
        let launch = (tick / phase) as usize % 4;
        let starting = tick % phase < phase / 5;
        let mut groups = vec![kernel, daemons, app(launch)];
        if starting {
            groups.push(start(launch));
        }
        let once = (0..ONCE_PER_TICK).map(|at| pool + (tick as usize * ONCE_PER_TICK + at) % POOL);
        let entered: Vec<(usize, u64, u64)> = groups
            .iter()
            .flat_map(|group| {
                (0..group.blocks)
                    .filter(|&rank| uniform(&mut seed) < odds[group.first + rank])
                    .map(|rank| (group.first + rank, group.fetches, group.transfers))
                    .collect::<Vec<_>>()
            })
            .chain(once.map(|block| (block, 0, 20_000)))
            .collect();
        for (block, fetches, transfers) in entered {
            if let Some(part) = model.part_of[block]
                && model.ring.open_part(model.ring.kind(part as usize)) != Some(part as usize)
                && uniform(&mut seed) < LATE_LINK
            {
                model.late_link(part as usize, late);
            }
            {
                let part = match model.part_of[block] {
                    Some(part) => part,
                    None => {
                        let part = model.install(block);
                        heat_of[part as usize] += INSTALL_FETCHES as f64;
                        part
                    }
                };
                // The engine's fetches: every entry of exiting code, one in
                // a run's budget of chained code.
                let mut entries = fetches as f64 + transfers as f64 / CHAIN_LIMIT as f64;
                if heat == Heat::Sampled {
                    let samples = transfers as f64 / SAMPLE_WEIGHT as f64;
                    if uniform(&mut seed) < samples {
                        entries += SAMPLE_WEIGHT as f64;
                    }
                }
                heat_of[part as usize] += entries;
            }
        }
        // PEs add their counts in batches, before installs: once a tick.
        for (part, heat) in heat_of.iter_mut().enumerate() {
            let whole = heat.floor();
            if whole > 0.0 {
                model.ring.entered_by(part, whole as u64);
                *heat -= whole;
            }
        }
    }
    model.outcome.pinned = model.ring.late_counts()[2];
    model.outcome
}

/// The replay (`cargo test -p stela-engine --lib churn_replay --
/// --nocapture` prints it). With late thunks and tails in the open part,
/// the cold choice can take only the oldest tenured parts, whatever their
/// heat; in their own part's late room the choice is free, and with heat
/// from sampled native transfers it keeps the parts of chained code an app
/// keeps running, so fewer of its blocks come back. Kept forms turn some of
/// the returns into installs without a compile.
#[test]
fn churn_replay_keeps_chained_hot_code_and_kept_forms_save_compiles() {
    const TICKS: u64 = 1600;
    let run = |heat, late, kept| replay(Setup { heat, late, kept }, TICKS);
    let before = run(Heat::Fetches, false, 0);
    let sampled_pinned = run(Heat::Sampled, false, 0);
    let late = run(Heat::Fetches, true, 0);
    let both = run(Heat::Sampled, true, 0);
    let kept = run(Heat::Sampled, true, 2500);
    for (name, outcome) in [
        ("fetches+pins", before),
        ("sampled+pins", sampled_pinned),
        ("fetches+late", late),
        ("sampled+late", both),
        ("sampled+late+kept", kept),
    ] {
        std::println!("CHURN REPLAY {name} {outcome:?}");
    }
    assert!(before.again > 0 && before.retired > 0);
    assert!(before.pinned > 0 && both.pinned < before.pinned);
    assert!(
        both.promoted * 100 <= before.promoted * 85,
        "sampled heat and late rooms {} against before {}",
        both.promoted,
        before.promoted
    );
    assert!(both.promoted < late.promoted && both.promoted < sampled_pinned.promoted);
    assert!(kept.kept_hits > 0);
    assert!(kept.compiles < both.compiles);
}
