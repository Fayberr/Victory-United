//! Which title memory the title wrote since a host copy took it (decision
//! 0007, "Copies of title memory"). The runtime reserves title GPU memory
//! with write watching (`victory-hle` xmem), so the kernel keeps a dirty bit
//! per 4 KiB page; asking for a range's written pages also clears them. Host
//! copies of the same memory can overlap (nested buffers, textures in
//! buffers), so what one asks is kept here for the others: a journal of
//! each page's last write, numbered by the asking.
//!
//! Physical-page views (xmem's sections) cannot be write-watched. There a
//! copy leaves each 64 KiB block it took read-only, and our vectored handler
//! takes the first write to it: it notes the block as written and gives the
//! write access back. The runtime tells us when it maps or unmaps views
//! ([`title_memory_remapped`]), which resets their protection. Blocks found
//! wholly uncommitted are skipped while they remain so; one query can
//! confirm a whole region, including ordinary reservations between views.
//!
//! Writes into physical pages through another view are seen too: the
//! runtime tells us the views of the pages it wrote itself ([`writing`],
//! xmem's `write_physical` and DirectStorage's page destinations), and a
//! write by the title through any view of a guarded page faults like one
//! at the guarded address (the guard makes every view of the page
//! read-only, [`open`] gives them all back; not a page of more than
//! [`MANY_VIEWS`] views). A
//! system call writing into a guarded block fails (none is known to).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, RwLock};

/// The unit tracked and copied: 64 KiB, the console's page (the kernel's
/// 4 KiB pages fold into it). Finer units made the checks of the world's
/// big buffers cost more than the copies they saved.
pub(crate) const PAGE: usize = 64 << 10;
/// The kernel's page, the unit of its answers.
const KERNEL_PAGE: usize = 4096;

/// The parts of a page.
const PARTS: usize = PAGE / KERNEL_PAGE;
/// All of them.
const WHOLE: u16 = u16::MAX;

/// Whether the parts of a page that the kernel names are kept, and a refresh
/// copies those (`VICTORY_PAGE_PARTS=0`: the 64 KiB page, as before).
///
/// The kernel says which 4 KiB pages were written, and a copy took the 64 KiB
/// around each: 29 MB a frame written so were most of 114 MB copied, and as
/// much sent up and hashed again on the card (session play-1125-0203, run
/// cpu-2). What is checked is as before, a page.
fn parts_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PAGE_PARTS").is_none_or(|v| v != "0"))
}

/// A title-memory range whose changed pages another subsystem needs to read.
pub(crate) struct Tracked {
    start: usize,
    end: usize,
    seen: Mutex<BTreeMap<usize, u64>>,
}

/// Tracks a runtime GPU-memory range, including physical-page views when
/// `views` is true. Other process memory is not tracked.
pub(crate) fn track(start: usize, len: usize, views: bool) -> Option<Tracked> {
    let end = start.checked_add(len)?;
    if len == 0 || writes(start, end, views).is_none() {
        return None;
    }
    Some(Tracked {
        start,
        end,
        seen: Mutex::default(),
    })
}

/// Strictly monotonic asking counter so clean queries can stay under a shared
/// `JOURNAL.read()` lock while preserving `later > now` ordering.
static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_asking() -> u64 {
    ASKED.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1
}

struct Journal {
    /// Page number → the asking that last found it written.
    written: BTreeMap<usize, u64>,
    /// The same for each of a page's sixteen parts ([`KERNEL_PAGE`]), where
    /// the parts are known ([`parts_on`]); a page that is not here was
    /// written whole, as far as anyone knows.
    parts: std::collections::HashMap<usize, [u64; PARTS], std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>,
    /// Room for the kernel's answer.
    found: Vec<usize>,
    /// Page ranges asked ahead for a submission's refreshes ([`ask_ahead`])
    /// and the asking that did: `writes` answers for a range inside one
    /// from the journal, without another kernel call. In order and joined
    /// ([`add_range`]): two ranges asked about next to each other answer for
    /// a range over both.
    ahead: Vec<(usize, usize)>,
    /// As `ahead`, of view memory the guards were asked about with the
    /// submission's one asking ([`ask_views`]): answers only askings about
    /// guarded memory (`writes` with `guarded`).
    ahead_views: Vec<(usize, usize)>,
    ahead_asked: u64,
    /// The ahead asking stands for a whole submission ([`ask_once`]): no
    /// other ahead asking is made, and `ahead` stays, until
    /// [`ask_once_end`].
    kept: bool,
}

/// Shared by those the journal answers without an asking ([`writes_into`],
/// [`asked`]): the refreshes of a submission run on several threads
/// (`crate::host` `wide_refresh`) and each asked here, one after the other.
/// Alone for askings and for what is told (after reddxd-byte/ShakiraRED-dt
/// c2271e5).
static JOURNAL: std::sync::RwLock<Journal> = std::sync::RwLock::new(Journal {
    written: BTreeMap::new(),
    parts: std::collections::HashMap::with_hasher(std::hash::BuildHasherDefault::new()),
    found: Vec::new(),
    ahead: Vec::new(),
    ahead_views: Vec::new(),
    ahead_asked: 0,
    kept: false,
});

impl Journal {
    /// Page `page` was found written by asking `now`: the parts in `parts`
    /// (a bit each). The asking that found it written before (0: none).
    fn note(&mut self, page: usize, parts: u16, now: u64) -> u64 {
        let before = self.written.insert(page, now).unwrap_or(0);
        if parts == WHOLE || !parts_on() {
            self.parts.remove(&page);
            return before;
        }
        let all = self.parts.entry(page).or_insert([before; PARTS]);
        for (k, at) in all.iter_mut().enumerate() {
            if parts >> k & 1 != 0 {
                *at = now;
            }
        }
        before
    }

    /// Whether the standing asking holds pages `[first, last)`.
    fn stands(&self, first: usize, last: usize, guarded: bool) -> bool {
        covered(&self.ahead, first, last) || guarded && covered(&self.ahead_views, first, last)
    }

    /// The parts of `page` written by an asking after `taken`.
    fn parts_since(&self, page: usize, taken: u64) -> u16 {
        match self.parts.get(&page) {
            Some(all) => all.iter().enumerate().filter(|(_, at)| **at > taken).fold(0, |m, (k, _)| m | 1 << k),
            None => WHOLE,
        }
    }
}

/// Of the pages of a copy that were written after it took them (page number,
/// the asking it took the page at): the parts to take again, a bit for each
/// 4 KiB, in the same order.
pub(crate) fn parts_written(pages: &[(usize, u64)]) -> Vec<u16> {
    let j = JOURNAL.read().unwrap_or_else(|e| e.into_inner());
    pages.iter().map(|&(page, taken)| j.parts_since(page, taken)).collect()
}

/// Ranges of one submission closer than this are asked about together:
/// walking the pages between them costs less than another kernel call (a
/// call that resets written pages costs 30-70 microseconds whatever the
/// range).
const AHEAD_GAP: usize = (8 << 20) / PAGE;
/// The most one asking covers (its answer needs 8 bytes per 4 KiB page).
const AHEAD_MAX: usize = (1 << 30) / PAGE;

/// Asks the kernel about many ranges `(start, end)` at once, before the
/// [`writes`] of each: ranges close together are one asking, and `writes`
/// then answers from the journal until [`ask_ahead_done`]. The world's
/// submissions refresh thousands of host copies (6,664 askings a frame,
/// 456 ms, in run 14). Ranges that are not write-watched, or lie alone, are
/// left to `writes`.
pub(crate) fn ask_ahead(ranges: &[(usize, usize)]) {
    let _gate = GATE.write().unwrap_or_else(|e| e.into_inner());
    ask_ahead_gated(ranges);
}

/// [`ask_ahead`] for a caller that holds [`GATE`] for writing.
fn ask_ahead_gated(ranges: &[(usize, usize)]) {
    let mut pieces: Vec<(usize, usize)> = ranges
        .iter()
        .filter(|r| r.0 < r.1)
        .map(|&(start, end)| (start / PAGE, end.div_ceil(PAGE)))
        .collect();
    if pieces.len() < 2 {
        return;
    }
    pieces.sort_unstable();
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    let j = &mut *j;
    // The submission's one asking stands ([`ask_once`]): what it does not
    // hold is left to `writes`.
    if j.kept {
        return;
    }
    // View memory the guards hold is left to `writes`, which answers it
    // from the guards ([`known_view`]): the kernel's write watch can only
    // fail there. Asked here with the others, a run holding such a range
    // failed, was asked again by halves down to single ranges, failing
    // each time, and the range then went to `writes` all the same (after
    // triippywhale d0581a55). `VICTORY_VIEW_ASK=1` asks as before.
    #[cfg(feature = "diagnostics")]
    let all = pieces.len();
    pieces.retain(|&(first, _)| !known_view(first * PAGE));
    #[cfg(feature = "diagnostics")]
    AHEAD_VIEWS.fetch_add((all - pieces.len()) as u64, std::sync::atomic::Ordering::Relaxed);
    j.ahead_asked = next_asking();
    j.ahead.clear();
    let mut i = 0;
    while i < pieces.len() {
        let first = pieces[i].0;
        let mut last = pieces[i].1;
        let mut k = i + 1;
        while k < pieces.len()
            && pieces[k].0 <= last + AHEAD_GAP
            && last.max(pieces[k].1) - first <= AHEAD_MAX
        {
            last = last.max(pieces[k].1);
            k += 1;
        }
        ask_run(j, &pieces[i..k]);
        i = k;
    }
}

/// Since the last frame line: the kernel askings that took a millisecond
/// or more, their microseconds, and the longest asking's. An asking walks
/// page tables, some ten microseconds for most ranges; in run 75 the 2,644
/// askings of a frame took 219 ms in the lines where the pass over the
/// heap's views ran long, 25-50 ms in the others. These numbers say
/// whether a few askings wait for something or all of them are slow.
#[cfg(feature = "diagnostics")]
static SLOW: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

#[cfg(feature = "diagnostics")]
fn slow(us: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    if us >= 1000 {
        SLOW[0].fetch_add(1, Relaxed);
        SLOW[1].fetch_add(us, Relaxed);
    }
    SLOW[2].fetch_max(us, Relaxed);
}

/// Since the last frame line: ranges of view memory the guards hold that a
/// submission's askings ahead ([`ask_ahead`], [`ask_once`]) left out. Each
/// was a kernel asking that failed, or several, before.
#[cfg(feature = "diagnostics")]
pub(crate) static AHEAD_VIEWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`SLOW`], taken.
#[cfg(feature = "diagnostics")]
pub(crate) fn slow_asks() -> [u64; 3] {
    [0, 1, 2].map(|i| SLOW[i].swap(0, std::sync::atomic::Ordering::Relaxed))
}

/// The submission's refreshes are done: `writes` asks the kernel again.
pub(crate) fn ask_ahead_done() {
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    if !j.kept {
        j.ahead.clear();
    }
}

/// Whether a submission asks the kernel once, before its first refresh,
/// about every range its lists refresh (`VICTORY_ASK_ONCE=1`; off by
/// default).
///
/// A submission's refreshes are done in groups, between the title's own
/// buffer copies (each copy flushes the refreshes recorded before it), and
/// every group asked the kernel again: 2,644 askings a frame, 70 a
/// submission. With the switch the journal answers all of them from one
/// asking made when the submission starts.
///
/// What changes: memory the title's other threads write while the
/// submission is being prepared is seen by the next submission's asking,
/// not by this one's later groups. What this submission's lists read was
/// written before they were submitted, so before the asking. Our own
/// writes into title memory while preparing are told to the journal
/// ([`wrote`]) or end the asking ([`ask_once_end`]).
pub(crate) fn once() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // On by default since run 81 (with card buffers: 375-450 ms a frame
    // against 573-647, the picture unchanged over four minutes);
    // `VICTORY_ASK_ONCE=0` asks per range, as before.
    *ON.get_or_init(|| std::env::var_os("VICTORY_ASK_ONCE").is_none_or(|v| v != "0"))
}

/// Whether the submission's one asking ([`once`]) is of the guards too
/// (`VICTORY_ASK_ONCE_VIEWS`; off by default): 1, of the view memory its
/// refreshes of buffers take; 2, also of the memory of the textures it
/// checks; `alt`: twenty seconds with 1, twenty without, in turn, each
/// change said in the log, for measuring it in one scene. After
/// reddxd-byte/ShakiraRED-dt c2271e5, where it has no switch and is 2.
///
/// The asking left view memory to the guards, which were asked anew by each
/// copy at each flush, one thread at a time (the journal and the guards are
/// locked over it): the copies a submission refreshes on several threads
/// ([`crate::host`] `wide_refresh`) stood in line for it. Asked once, they
/// look the answer up side by side.
///
/// What changes is what [`once`] says of write-watched memory: what the
/// title's other threads write into a view while the submission is being
/// prepared, or show there anew, is seen by the next submission.
pub(crate) fn ask_views() -> u8 {
    /// 0 off, 1 buffers, 2 textures too, 3 in turn.
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    match *MODE.get_or_init(|| match std::env::var("VICTORY_ASK_ONCE_VIEWS").as_deref().map(str::trim) {
        Ok("1") => 1,
        Ok("2") => 2,
        Ok("alt") => 3,
        _ => 0,
    }) {
        3 => {
            static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            let on = (EPOCH.get_or_init(std::time::Instant::now).elapsed().as_secs() / 20) % 2 == 1;
            static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
            if LAST.swap(u8::from(on), std::sync::atomic::Ordering::Relaxed) != u8::from(on) {
                tracing::info!(
                    "the guards are asked once a submission from now (VICTORY_ASK_ONCE_VIEWS=alt): {}",
                    if on { "ON" } else { "off, as usual" }
                );
            }
            u8::from(on)
        }
        mode => mode,
    }
}

/// With [`once`]: asks about `ranges` (everything a submission's lists
/// refresh) as [`ask_ahead`] does, and lets that asking stand until
/// [`ask_once_end`].
pub(crate) fn ask_once(ranges: &[(usize, usize)]) {
    SUBMISSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if once() {
        ask_now(ranges, ask_views() > 0);
    }
}

/// [`ask_once`]; with `views` the guards are asked too ([`ask_views`]).
fn ask_now(ranges: &[(usize, usize)], views: bool) {
    let _gate = GATE.write().unwrap_or_else(|e| e.into_inner());
    ask_once_end();
    ask_ahead_gated(ranges);
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    let j = &mut *j;
    if j.ahead.is_empty() {
        // `ask_ahead` asked nothing (fewer than two ranges).
        j.ahead_asked = next_asking();
    }
    // `ask_ahead` leaves a range that lies alone to `writes`, which asks
    // once per flush: with one asking for the submission that was every
    // such range asked again at each of its 70 flushes (run 76: 2,300
    // askings a frame with the switch on, as without it). They are asked
    // here, once; what is not write-watched stays with `writes`.
    for &(start, end) in ranges {
        if start >= end {
            continue;
        }
        let (first, last) = (start / PAGE, end.div_ceil(PAGE));
        // View memory the guards hold is not asked about: the asking can
        // only fail there, one failed kernel call for each such copy at
        // every submission, and `writes` answers it from the guards
        // (after triippywhale cc368c7c).
        if known_view(start) {
            #[cfg(feature = "diagnostics")]
            AHEAD_VIEWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // The guards are asked here, once, with [`ask_views`].
            if views
                && !covered(&j.ahead_views, first, last)
                && let Some(written) = guard(first * PAGE, last * PAGE, true)
            {
                let now = j.ahead_asked;
                for page in written {
                    j.written.insert(page, now);
                }
                add_range(&mut j.ahead_views, first, last);
            }
            continue;
        }
        if !covered(&j.ahead, first, last) && ask_kernel(j, first, last) {
            add_range(&mut j.ahead, first, last);
        }
    }
    j.kept = !(j.ahead.is_empty() && j.ahead_views.is_empty());
}

/// The submission's one asking stands no longer: its refreshes are done,
/// or title memory was written in a way the journal was not told.
pub(crate) fn ask_once_end() {
    crate::stamp::advance();
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    if j.kept {
        j.kept = false;
        j.ahead.clear();
        j.ahead_views.clear();
    }
}

/// We wrote title memory `[at, at + len)` while a submission's one asking
/// stands (the title's buffer copies, done at submission): its pages count
/// as written after that asking, so the refreshes that follow copy them.
pub(crate) fn wrote(at: usize, len: usize) {
    // Copies checked over it check again (`crate::stamp`).
    crate::stamp::advance();
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    if !j.kept || len == 0 {
        return;
    }
    let now = next_asking();
    // The asking that stands moves with it: `asked` and `writes` give it as
    // "now", and a page noted written after "now" stays stale for every
    // copy that takes it (copied at `now`, written at `now + 1`): copied
    // again at every refresh, and a reason to settle flights
    // (fxceless/victory-fork#410).
    j.ahead_asked = now;
    for page in at / PAGE..at.saturating_add(len).div_ceil(PAGE) {
        j.note(page, WHOLE, now);
    }
}

/// We wrote the 64 KiB page numbered `page` of title memory through a
/// view of our own (`crate::inplace::write_own`): no guard was opened and
/// no kernel write-watch bit set, so the journal is told here. The page,
/// and the same physical page at its other title addresses (`others`,
/// page numbers), count as written at a new asking, which every host copy
/// that holds them takes at its next refresh. Returns the asking that had
/// last found `page` written (0: never) and the new one, for the copy
/// that wrote to note itself current.
pub(crate) fn wrote_own(page: usize, others: &[usize]) -> (u64, u64) {
    crate::stamp::advance();
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    let j = &mut *j;
    let now = next_asking();
    // As in [`wrote`]: an asking that stands for a submission, or for a
    // flush, moves with it, or a copy taking the page at the standing
    // asking would find it written "later" at every refresh.
    if j.kept || !j.ahead.is_empty() {
        j.ahead_asked = now;
    }
    let before = j.note(page, WHOLE, now);
    for &other in others {
        if other != page {
            j.note(other, WHOLE, now);
        }
    }
    (before, now)
}

/// The asking that last found the 64 KiB page numbered `page` written (0:
/// never). Read only, for `crate::backreads`.
#[cfg(feature = "diagnostics")]
pub(crate) fn stamp(page: usize) -> u64 {
    JOURNAL.read().unwrap_or_else(|e| e.into_inner()).written.get(&page).copied().unwrap_or(0)
}

/// One kernel asking for `pieces` (page ranges in order); by halves if it
/// fails, as a run across two reservations or into memory that is not
/// write-watched does. A piece alone is left to [`writes`].
fn ask_run(j: &mut Journal, pieces: &[(usize, usize)]) {
    if pieces.len() < 2 {
        return;
    }
    let first = pieces[0].0;
    let last = pieces.iter().map(|p| p.1).max().unwrap_or(first);
    if ask_kernel(j, first, last) {
        add_range(&mut j.ahead, first, last);
        return;
    }
    let (a, b) = pieces.split_at(pieces.len() / 2);
    ask_run(j, a);
    ask_run(j, b);
}

/// Asks the kernel for the written pages of pages `[first, last)` and notes
/// them in the journal under the ahead asking; false (and nothing changed)
/// if the range is not write-watched memory of one reservation.
fn ask_kernel(j: &mut Journal, first: usize, last: usize) -> bool {
    use windows_sys::Win32::System::Memory::GetWriteWatch;
    const WRITE_WATCH_FLAG_RESET: u32 = 1;
    let mut count = (last - first) * PAGE / KERNEL_PAGE;
    j.found.resize(count.max(j.found.len()), 0);
    let mut granularity = 0;
    #[cfg(feature = "diagnostics")]
    let started = std::time::Instant::now();
    // SAFETY: the answer buffer holds `count` entries.
    let failed = unsafe {
        GetWriteWatch(
            WRITE_WATCH_FLAG_RESET,
            (first * PAGE) as *const _,
            (last - first) * PAGE,
            j.found.as_mut_ptr().cast(),
            &mut count,
            &mut granularity,
        )
    } != 0;
    if failed {
        return false;
    }
    #[cfg(feature = "diagnostics")]
    {
        use crate::copystat::{C, add};
        add(C::Watches, 1);
        add(C::WatchUs, started.elapsed().as_micros() as u64);
        slow(started.elapsed().as_micros() as u64);
    }
    let now = j.ahead_asked;
    // The parts the kernel names, as `writes_kind` keeps them.
    let mut pages: Vec<(usize, u16)> = Vec::with_capacity(count.min(4096));
    for k in 0..count {
        let at = j.found[k];
        let (page, part) = (at / PAGE, 1u16 << (at % PAGE / KERNEL_PAGE));
        match pages.last_mut() {
            Some((last, parts)) if *last == page => *parts |= part,
            _ => pages.push((page, part)),
        }
    }
    for (page, parts) in pages {
        j.note(page, parts, now);
    }
    true
}

/// What [`writes`] would say of `[start, end)` from the journal alone, if
/// the range lies in one asked ahead ([`ask_ahead`], [`ask_once`]): the
/// pages written since tracking began and the asking that last found each
/// written, with the number of the asking that stands (as [`writes`] gives
/// it). `None` if the kernel would have to be asked.
pub(crate) fn asked(start: usize, end: usize) -> Option<(u64, Vec<(usize, u64)>)> {
    let (first, last) = (start / PAGE, end.div_ceil(PAGE));
    let j = JOURNAL.read().unwrap_or_else(|e| e.into_inner());
    j.stands(first, last, true)
        .then(|| (j.ahead_asked, j.written.range(first..last).map(|(&p, &w)| (p, w)).collect()))
}

/// The pages of `[start, end)` (whole pages) as last written, for pages
/// written since tracking began: `(page number, asking)` in page order, and
/// this asking's number, which is higher than any in the list but those the
/// kernel just reported. `None` if the memory is not write-watched (nor,
/// with `guarded`, view memory, which is then guarded).
pub(crate) fn writes(start: usize, end: usize, guarded: bool) -> Option<(u64, Vec<(usize, u64)>)> {
    let mut written = Vec::new();
    writes_into(start, end, guarded, &mut written).map(|now| (now, written))
}

/// [`writes`] with the list put into `written` (emptied first) and the
/// asking's number returned: for the submitting thread's refreshes, which
/// ask some 20,000 times a frame and keep the room between askings (a new
/// list each time, grown entry by entry, was a tenth of that thread's
/// samples in run int-b).
pub(crate) fn writes_into(
    start: usize,
    end: usize,
    guarded: bool,
    written: &mut Vec<(usize, u64)>,
) -> Option<u64> {
    writes_inner(start, end, guarded, written).map(|(now, _)| now)
}

/// [`writes_into`], also saying whether the answer came from the guards
/// (view memory), for [`writes_kind`].
fn writes_inner(
    start: usize,
    end: usize,
    guarded: bool,
    written: &mut Vec<(usize, u64)>,
) -> Option<(u64, bool)> {
    use windows_sys::Win32::System::Memory::GetWriteWatch;
    const WRITE_WATCH_FLAG_RESET: u32 = 1;
    written.clear();
    let (first, last) = (start / PAGE, end.div_ceil(PAGE));
    // Asked ahead with the submission's other ranges: the journal has it.
    {
        let j = JOURNAL.read().unwrap_or_else(|e| e.into_inner());
        if j.stands(first, last, guarded) {
            written.extend(j.written.range(first..last).map(|(&p, &w)| (p, w)));
            return Some((j.ahead_asked, false));
        }
    }
    if apart() {
        return writes_apart(start, end, guarded, written);
    }
    let mut journal = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    let j = &mut *journal;
    if j.stands(first, last, guarded) {
        // Asked about between the two looks.
        written.extend(j.written.range(first..last).map(|(&p, &w)| (p, w)));
        return Some((j.ahead_asked, false));
    }
    #[cfg(feature = "diagnostics")]
    let started = std::time::Instant::now();
    // View memory the guards already hold cannot be write-watched: the
    // kernel asking would only fail. Copies of it are flushed many times a
    // frame, each time paying that failed call before the guards answer.
    let view = guarded && known_view(first * PAGE);
    let mut count = 0;
    let failed = view || {
        count = (last - first) * PAGE / KERNEL_PAGE;
        j.found.resize(count.max(j.found.len()), 0);
        let mut granularity = 0;
        // SAFETY: the answer buffer holds `count` entries.
        let status = unsafe {
            GetWriteWatch(
                WRITE_WATCH_FLAG_RESET,
                (first * PAGE) as *const _,
                (last - first) * PAGE,
                j.found.as_mut_ptr().cast(),
                &mut count,
                &mut granularity,
            )
        };
        status != 0
    };
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(
        if view {
            crate::copystat::C::WatchKnownView
        } else {
            crate::copystat::C::WatchFailed
        },
        failed as u64,
    );
    #[cfg(feature = "diagnostics")]
    if failed && !view && guarded {
        failed_asking(first * PAGE, last * PAGE);
    }
    let guarded = if failed {
        count = 0;
        if !guarded {
            return None;
        }
        #[cfg(feature = "diagnostics")]
        let guarding = std::time::Instant::now();
        let pages = guard(first * PAGE, last * PAGE, view);
        #[cfg(feature = "diagnostics")]
        crate::copystat::add(crate::copystat::C::GuardUs, guarding.elapsed().as_micros() as u64);
        if view && pages.is_none() {
            // A hole it was, and private memory now ([`guard`] has forgotten
            // it): asked about as any other memory.
            drop(journal);
            return writes_into(start, end, guarded, written);
        }
        pages?
    } else {
        Vec::new()
    };
    #[cfg(feature = "diagnostics")]
    {
        use crate::copystat::{C, add};
        add(C::Watches, 1);
        add(C::WatchUs, started.elapsed().as_micros() as u64);
        slow(started.elapsed().as_micros() as u64);
    }
    let now = next_asking();
    // The 4 KiB parts the kernel names, not the 64 KiB around them: a
    // refresh copies those (`parts_written`).
    let mut parts: Vec<(usize, u16)> = Vec::with_capacity(count.min(4096));
    for k in 0..count {
        let at = j.found[k];
        let (page, part) = (at / PAGE, 1u16 << (at % PAGE / KERNEL_PAGE));
        match parts.last_mut() {
            Some((last, held)) if *last == page => *held |= part,
            _ => parts.push((page, part)),
        }
    }
    for (page, held) in parts {
        j.note(page, held, now);
    }
    for page in guarded {
        j.note(page, WHOLE, now);
    }
    written.extend(j.written.range(first..last).map(|(&p, &w)| (p, w)));
    Some((now, failed))
}

/// Direct path for memory already known to be view-guarded: skips the
/// failing `GetWriteWatch` kernel syscall where the guards say so and runs
/// `guard()` for the pages.
pub(crate) fn writes_guarded(start: usize, end: usize) -> Option<(u64, Vec<(usize, u64)>)> {
    let mut written = Vec::new();
    writes_into(start, end, true, &mut written).map(|now| (now, written))
}

/// Like [`writes`], also returning whether the range is view-guarded memory
/// (`true`) so callers can cache that fact and skip future `GetWriteWatch`
/// syscalls via [`writes_guarded`].
pub(crate) fn writes_kind(start: usize, end: usize, guarded: bool) -> Option<(u64, Vec<(usize, u64)>, bool)> {
    let mut written = Vec::new();
    writes_inner(start, end, guarded, &mut written).map(|(now, view)| (now, written, view))
}

/// Whether askings about different pages go to the kernel side by side
/// (`VICTORY_WATCH_APART=1`; off by default). Measured in run 27 (open
/// world, diagnostics build): the submitting thread stood 20 ms a frame,
/// and the wide workers 209 ms between them, waiting for the journal's
/// lock, which [`writes_into`] held across the kernel's answer and the
/// guards'. Here only the pages in question are held ([`Claim`]), and the
/// journal only while the answer goes in.
fn apart() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_WATCH_APART").is_some_and(|v| v == "1"))
}

/// Askings about pages under way side by side ([`writes_apart`]) hold it for
/// reading; one that asks for a submission's ranges and makes them stand
/// ([`ask_ahead`], [`ask_once`]) holds it for writing, so that no answer is
/// missing from the journal when it stands. Taken before [`JOURNAL`].
static GATE: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// The 64 KiB pages (also the guards' blocks) being asked about.
static IN_FLIGHT: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());
static LANDED: std::sync::Condvar = std::sync::Condvar::new();

/// Pages `[first, last)` held for one asking: the kernel's answer clears the
/// written state it reports, and a guard taken again forgets it, so a
/// second asking about one of these pages before the first answer is in
/// the journal would see neither. Askings about other pages go on.
struct Claim(usize, usize);

impl Claim {
    fn take(first: usize, last: usize) -> Claim {
        let mut held = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        while held.iter().any(|&(a, b)| a < last && first < b) {
            held = LANDED.wait(held).unwrap_or_else(|e| e.into_inner());
        }
        held.push((first, last));
        Claim(first, last)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut held = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = held.iter().position(|&r| r == (self.0, self.1)) {
            held.swap_remove(at);
        }
        drop(held);
        LANDED.notify_all();
    }
}

/// What the kernel or the guards said of pages `[first, last)`.
enum Answer {
    /// Kernel pages (addresses) and guarded pages (64 KiB page numbers).
    Written(Vec<usize>, Vec<usize>),
    /// Neither watched nor a view.
    Unwatched,
    /// A view block turned out to be private memory: ask again.
    Again,
}

/// The kernel's and the guards' answer for `[first, last)`, without the
/// journal (as [`writes_into`] asks them).
fn answer(first: usize, last: usize, guarded: bool) -> Answer {
    use windows_sys::Win32::System::Memory::GetWriteWatch;
    const WRITE_WATCH_FLAG_RESET: u32 = 1;
    let view = guarded && known_view(first * PAGE);
    let mut found: Vec<usize> = Vec::new();
    let failed = view || {
        let mut count = (last - first) * PAGE / KERNEL_PAGE;
        found.resize(count, 0);
        let mut granularity = 0;
        // SAFETY: the answer buffer holds `count` entries.
        let status = unsafe {
            GetWriteWatch(
                WRITE_WATCH_FLAG_RESET,
                (first * PAGE) as *const _,
                (last - first) * PAGE,
                found.as_mut_ptr().cast(),
                &mut count,
                &mut granularity,
            )
        };
        found.truncate(count);
        status != 0
    };
    if !failed {
        return Answer::Written(found, Vec::new());
    }
    if !guarded {
        return Answer::Unwatched;
    }
    match guard(first * PAGE, last * PAGE, view) {
        Some(pages) => Answer::Written(Vec::new(), pages),
        None if view => Answer::Again,
        None => Answer::Unwatched,
    }
}

/// The journal told what `kernel` (addresses) and `pages` (page numbers)
/// say was written, at a new asking; what it holds of `[first, last)` put
/// into `written`; that asking's number.
fn note(first: usize, last: usize, kernel: Vec<usize>, pages: Vec<usize>, written: &mut Vec<(usize, u64)>) -> u64 {
    let mut j = JOURNAL.write().unwrap_or_else(|e| e.into_inner());
    let now = next_asking();
    // The 4 KiB parts the kernel names, not the 64 KiB around them: a
    // refresh copies those (`parts_written`).
    let mut parts: Vec<(usize, u16)> = Vec::with_capacity(kernel.len());
    for at in kernel {
        let (page, part) = (at / PAGE, 1u16 << (at % PAGE / KERNEL_PAGE));
        match parts.last_mut() {
            Some((last, held)) if *last == page => *held |= part,
            _ => parts.push((page, part)),
        }
    }
    for (page, held) in parts {
        j.note(page, held, now);
    }
    for page in pages {
        j.note(page, WHOLE, now);
    }
    written.extend(j.written.range(first..last).map(|(&p, &w)| (p, w)));
    now
}

/// Tests: microseconds put between the kernel's answer and the journal.
#[cfg(test)]
static WIDEN: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// [`writes_into`] with only the pages in question held while the kernel
/// and the guards answer ([`apart`]).
fn writes_apart(start: usize, end: usize, guarded: bool, written: &mut Vec<(usize, u64)>) -> Option<(u64, bool)> {
    let (first, last) = (start / PAGE, end.div_ceil(PAGE));
    // A second time if a view block turned out to be private memory (the
    // guards have forgotten it, so it is asked about as other memory), as
    // [`writes_into`] asks again.
    for _ in 0..2 {
        let _gate = GATE.read().unwrap_or_else(|e| e.into_inner());
        let _claim = Claim::take(first, last);
        {
            // Asked about while this waited for its pages.
            let j = JOURNAL.read().unwrap_or_else(|e| e.into_inner());
            if j.stands(first, last, guarded) {
                written.extend(j.written.range(first..last).map(|(&p, &w)| (p, w)));
                return Some((j.ahead_asked, false));
            }
        }
        let said = answer(first, last, guarded);
        // Tests widen the time between the answer and the journal, where a
        // second asking about the same pages would miss what this one found.
        #[cfg(test)]
        if let Some(us) = WIDEN.get() {
            std::thread::sleep(std::time::Duration::from_micros(*us));
        }
        match said {
            Answer::Written(kernel, pages) => {
                let view = !pages.is_empty();
                return Some((note(first, last, kernel, pages, written), view));
            }
            Answer::Unwatched => return None,
            Answer::Again => {}
        }
    }
    None
}

/// Diagnostics: one in 2,048 of the askings the kernel's write watch
/// refused for memory the guards did not know (some 200 a frame in the
/// first rooms, each followed by a question to Windows about the memory):
/// what the memory is and what the guards hold of its first block.
#[cfg(feature = "diagnostics")]
fn failed_asking(start: usize, end: usize) {
    use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 2048 != 0 {
        return;
    }
    // SAFETY: plain data filled in by the query.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer of the right size.
    let n = unsafe { VirtualQuery(start as *const _, &mut info, size_of_val(&info)) };
    let (protected, clean, absent, viewed, aliased) = {
        let g = guards_shared();
        let b = start / BLOCK;
        (g.protected.contains(&b), g.clean.contains(&b), g.absent.contains(&b), covered(&g.views, b, b + 1), g.aliased.contains(&b))
    };
    tracing::info!(
        "a failed write-watch asking (one in 2048): {start:#x}+{:#x}; Windows says (answered {n}): base {:#x} region {:#x} state {:#x} type {:#x} protect {:#x} allocation protect {:#x}; \
         the guards of its first block: protected {protected} clean {clean} absent {absent} viewed {viewed} aliased {aliased}; is a view: {}",
        end - start,
        info.BaseAddress as usize,
        info.RegionSize,
        info.State,
        info.Type,
        info.Protect,
        info.AllocationProtect,
        is_view(start, end)
    );
}

impl Tracked {
    /// Takes changed pages of `[from, to)`, cut to the tracked range. A
    /// partial page stays changed so a later take can read its other bytes.
    pub(crate) fn take(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let (from, to) = (from.max(self.start), to.min(self.end));
        if from >= to {
            return Vec::new();
        }
        let Some((_, pages)) = writes(from, to, true) else {
            return Vec::new();
        };
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (page, write) in pages {
            if seen.get(&page).is_some_and(|&last| last >= write) {
                continue;
            }
            let (start, end) = ((page * PAGE).max(from), ((page + 1) * PAGE).min(to));
            if start >= end {
                continue;
            }
            // Keep partial pages dirty: the caller has not taken the rest.
            if (page * PAGE).max(self.start) >= from
                && ((page + 1) * PAGE).min(self.end) <= to
            {
                seen.insert(page, write);
            }
            match ranges.last_mut() {
                Some((_, last)) if *last == start => *last = end,
                _ => ranges.push((start, end)),
            }
        }
        ranges
    }
}

/// Guarded blocks of view memory.
const BLOCK: usize = PAGE;

/// Whether the block holding `start` is view memory the guards actively
/// hold read-only: asking the kernel's write watch about it can only fail.
/// Uncommitted `absent` blocks are excluded because the title may later
/// allocate `MEM_WRITE_WATCH` private memory at the same virtual address.
/// `VICTORY_VIEW_ASK=1` (or `VICTORY_GUARD_ASK=1`) asks the kernel anyway.
fn known_view(start: usize) -> bool {
    static ASK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let ask = *ASK.get_or_init(|| {
        ["VICTORY_VIEW_ASK", "VICTORY_GUARD_ASK"]
            .iter()
            .any(|n| std::env::var_os(n).is_some_and(|v| v == "1"))
    });
    if ask {
        return false;
    }
    let g = guards_shared();
    let first = start / BLOCK;
    !g.absent.contains(&first) && (g.protected.contains(&first) || (view_set() && covered(&g.views, first, first + 1)))
}

/// A set of blocks, a bit each, 64 to a word, the words by their number.
/// The guards ask whether all of a range is clean some 2,300 times a
/// frame, over 140,000 blocks in all: counted one by one in a tree of
/// blocks, that was most of the 18 ms a frame of "guards" (run int-c).
struct Blocks(BTreeMap<usize, u64>);

impl Blocks {
    const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// The bits of `range`'s blocks in word `word`.
    fn mask(word: usize, range: &std::ops::Range<usize>) -> u64 {
        let low = word * 64;
        let (from, to) = (range.start.max(low), range.end.min(low + 64));
        if from >= to {
            return 0;
        }
        (u64::MAX >> (64 - (to - from))) << (from - low)
    }

    fn insert(&mut self, block: usize) {
        *self.0.entry(block / 64).or_insert(0) |= 1 << (block % 64);
    }

    fn remove(&mut self, block: usize) {
        if let Some(bits) = self.0.get_mut(&(block / 64)) {
            *bits &= !(1 << (block % 64));
            if *bits == 0 {
                self.0.remove(&(block / 64));
            }
        }
    }

    fn contains(&self, block: usize) -> bool {
        self.0.get(&(block / 64)).is_some_and(|bits| bits >> (block % 64) & 1 != 0)
    }

    /// Takes `bits` of word `word` out of the set.
    fn clear(&mut self, word: usize, bits: u64) {
        if let Some(held) = self.0.get_mut(&word) {
            *held &= !bits;
            if *held == 0 {
                self.0.remove(&word);
            }
        }
    }

    fn extend(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() {
            return;
        }
        for word in range.start / 64..=(range.end - 1) / 64 {
            *self.0.entry(word).or_insert(0) |= Self::mask(word, &range);
        }
    }

    /// Whether every block of `range` is in the set.
    fn holds(&self, range: std::ops::Range<usize>) -> bool {
        if range.is_empty() {
            return true;
        }
        let (first, last) = (range.start / 64, (range.end - 1) / 64);
        let mut words = self.0.range(first..=last);
        (first..=last).all(|word| {
            let mask = Self::mask(word, &range);
            words.next().is_some_and(|(&at, &bits)| at == word && bits & mask == mask)
        })
    }

    /// The set's blocks in `range`, in order.
    fn range(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = usize> + '_ {
        let first = range.start / 64;
        let last = (range.end.max(range.start + 1) - 1) / 64;
        self.0.range(first..=last).flat_map(move |(&word, &bits)| {
            let mut left = bits & Self::mask(word, &range);
            std::iter::from_fn(move || {
                (left != 0).then(|| {
                    let bit = left.trailing_zeros() as usize;
                    left &= left - 1;
                    word * 64 + bit
                })
            })
        })
    }
}

struct Guards {
    /// Blocks we made read-only.
    protected: BTreeSet<usize>,
    /// Of those, the ones not written since a copy took them.
    clean: Blocks,
    /// Blocks already copied as zeros. While they remain wholly uncommitted,
    /// the copy can skip hundreds of MB of failing kernel reads per world frame.
    /// With [`holes_level`] 2 those also in `clean` were found uncommitted
    /// and hold as holes; the others are due a look.
    absent: Blocks,
    /// Blocks whose physical page was guarded together with its other views
    /// (those views' blocks are in it too): a write to one opens them all.
    /// A write range can be huge (`fread`), so only these are looked up.
    aliased: BTreeSet<usize>,
    /// Blocks known to be view memory ([`guard`]), in order and joined:
    /// a range in them needs no question to Windows to tell.
    views: Vec<(usize, usize)>,
    /// The blocks of `views` that are not clean (written since a copy took
    /// them, never taken, or not guardable), but for the parked ones: an
    /// asking whose range holds none of them is answered in one lookup,
    /// and otherwise walks only them ([`guard_unclean`]).
    unclean: BTreeSet<usize>,
    /// View blocks found uncommitted ([`Guards::absent`]), taken out of
    /// `unclean` while they stay so ([`park`]). One goes back when the
    /// runtime maps it ([`title_memory_remapped`]), when the title commits
    /// it through the runtime ([`title_memory_committed`]), or, if it is no
    /// placeholder, when the look at those every [`park_epoch`] submissions
    /// finds it committed (an ordinary commit no one tells of). The walk
    /// over every block of a range asked Windows about each at every
    /// asking: in the first room ~86,000-96,000 such blocks, nearly all
    /// 64 KiB placeholders, each its own region, and ~88,000 questions a
    /// frame (guards 26 ms of the submitting thread's frame on an RTX 5080;
    /// 14 ms parked).
    parked: BTreeSet<usize>,
    /// The parked blocks that are no placeholder: the ones looked at again.
    parked_ordinary: BTreeSet<usize>,
    /// The epoch ([`SUBMISSIONS`] / [`park_epoch`]) of the last look at
    /// `parked_ordinary`.
    parked_at: u64,
    /// With [`view_known`]: blocks made read-only at least once since the
    /// runtime last mapped or unmapped them. They are view memory whether
    /// their guard stands now or a write has opened it.
    viewed: BTreeSet<usize>,
    /// With [`holes_level`] 1 and 2: runs of uncommitted blocks as Windows
    /// described them in frame `holes_frame`, first block to end.
    holes: BTreeMap<usize, usize>,
    holes_frame: u64,
    /// With [`holes_level`] 2: the frame whose share of the holes was last
    /// made due ([`holes_due`]).
    due_frame: u64,
    /// With [`views_recheck`]: the frame whose share of the clean guarded
    /// blocks was last made due ([`views_due`]).
    views_frame: u64,
}

/// Whether a block once guarded stays known as view memory while its
/// guard is open (`VICTORY_VIEW_KNOWN=1`; off by default).
///
/// A range whose first block the title wrote since the last check has
/// that block neither read-only nor noted absent, so nothing says it is a
/// view: the kernel's write watch is asked and fails, and then Windows is
/// asked what the memory is (`is_view`), a walk of the page tables to the
/// end of the pages that share the first one's state. In runs 222 and 225
/// that was 338-450 failed askings a frame and 6.5-7.8 ms a frame of the
/// submitting thread in `VirtualQuery`. With the switch the block is
/// remembered from the time it was first made read-only until the runtime
/// maps or unmaps it ([`title_memory_remapped`]).
///
/// `VICTORY_VIEW_KNOWN=alt`: twenty seconds with, twenty without, in turn,
/// each change said in the log, for measuring it in one scene. (Blocks
/// noted in one phase are forgotten at a remapping in either.)
fn view_known() -> bool {
    /// 0 off, 1 on, 2 in turn.
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    match *MODE.get_or_init(|| match std::env::var("VICTORY_VIEW_KNOWN").as_deref() {
        Ok("1") => 1,
        Ok("alt") => 2,
        _ => 0,
    }) {
        0 => false,
        1 => true,
        _ => {
            static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            let on = (EPOCH.get_or_init(std::time::Instant::now).elapsed().as_secs() / 20) % 2 == 1;
            static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
            if LAST.swap(u8::from(on), std::sync::atomic::Ordering::Relaxed) != u8::from(on) {
                tracing::info!(
                    "view memory stays known while its guard is open from now (VICTORY_VIEW_KNOWN=alt): {}",
                    if on { "ON" } else { "off, as usual" }
                );
            }
            on
        }
    }
}

static GUARDS: std::sync::RwLock<Guards> = std::sync::RwLock::new(Guards {
    protected: BTreeSet::new(),
    clean: Blocks::new(),
    absent: Blocks::new(),
    aliased: BTreeSet::new(),
    views: Vec::new(),
    unclean: BTreeSet::new(),
    parked: BTreeSet::new(),
    parked_ordinary: BTreeSet::new(),
    parked_at: 0,
    viewed: BTreeSet::new(),
    holes: BTreeMap::new(),
    holes_frame: 0,
    due_frame: 0,
    views_frame: 0,
});

fn guards() -> std::sync::RwLockWriteGuard<'static, Guards> {
    GUARDS.write().unwrap_or_else(|e| e.into_inner())
}

/// The guards to look at only (after reddxd-byte/ShakiraRED-dt c2271e5):
/// what asks whether memory is guarded does not wait for others that ask.
fn guards_shared() -> std::sync::RwLockReadGuard<'static, Guards> {
    GUARDS.read().unwrap_or_else(|e| e.into_inner())
}

/// `[first, last)` added to `ranges` (in order, joined). For ranges seen
/// rarely (the views, once per new range): it sorts the whole list.
fn add_range(ranges: &mut Vec<(usize, usize)>, first: usize, last: usize) {
    if first >= last {
        return;
    }
    ranges.push((first, last));
    ranges.sort_unstable();
    let mut joined: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for &(a, b) in ranges.iter() {
        match joined.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => joined.push((a, b)),
        }
    }
    *ranges = joined;
}

/// Removes `[first, last)` from sorted, coalesced `ranges`.
fn remove_range(ranges: &mut Vec<(usize, usize)>, first: usize, last: usize) {
    if first >= last {
        return;
    }
    let at = ranges.partition_point(|r| r.1 <= first);
    if ranges.get(at).is_none_or(|r| r.0 >= last) {
        return;
    }
    let mut next: Vec<(usize, usize)> = Vec::with_capacity(ranges.len() + 1);
    for &(a, b) in ranges.iter() {
        if b <= first || a >= last {
            next.push((a, b));
        } else {
            if a < first {
                next.push((a, first));
            }
            if b > last {
                next.push((last, b));
            }
        }
    }
    *ranges = next;
}

/// Whether blocks `[first, last)` all lie in `ranges` (in order, joined).
fn covered(ranges: &[(usize, usize)], first: usize, last: usize) -> bool {
    let at = ranges.partition_point(|r| r.1 <= first);
    ranges.get(at).is_some_and(|r| r.0 <= first && last <= r.1)
}

/// Whether to keep views' not-clean blocks in a set and answer from it
/// ([`Guards::unclean`], [`Guards::parked`]; `VICTORY_VIEW_SET=0` walks
/// every block of a range, as before). Only where Windows tells
/// placeholders apart ([`placeholders_told`]).
fn view_set() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_VIEW_SET").is_none_or(|v| v != "0") && placeholders_told())
}

/// Whether [`in_placeholder`] knows a placeholder, asked once of one of
/// our own. Where it does not (Wine may not fill the flag), every parked
/// block would be looked at each epoch, more questions than the walk
/// asks: the walk stays there.
fn placeholders_told() -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_RELEASE, MEM_RESERVE, MEM_RESERVE_PLACEHOLDER, PAGE_NOACCESS, VirtualAlloc2, VirtualFree,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a fresh placeholder of one block, released at once.
    let p = unsafe {
        VirtualAlloc2(
            GetCurrentProcess(),
            std::ptr::null(),
            BLOCK,
            MEM_RESERVE | MEM_RESERVE_PLACEHOLDER,
            PAGE_NOACCESS,
            std::ptr::null_mut(),
            0,
        )
    } as usize;
    if p == 0 {
        return false;
    }
    let told = in_placeholder(p / BLOCK);
    // SAFETY: our own placeholder, known to nothing else.
    unsafe { VirtualFree(p as _, 0, MEM_RELEASE) };
    if !told {
        tracing::info!("Windows does not tell placeholders apart here: view memory askings walk every block (VICTORY_VIEW_SET)");
    }
    told
}

/// Since the last frame line ([`view_stats`]): view askings answered in one
/// lookup, view askings that walked their not-clean blocks, the blocks they
/// walked, the questions to Windows of whether a range is a view, the
/// questions whether blocks found uncommitted still are, and the parked
/// blocks found committed ([`Guards::parked`]).
static VIEW_STATS: [std::sync::atomic::AtomicU64; 6] = [const { std::sync::atomic::AtomicU64::new(0) }; 6];

/// [`VIEW_STATS`], taken.
pub(crate) fn view_stats() -> [u64; 6] {
    [0, 1, 2, 3, 4, 5].map(|i| VIEW_STATS[i].swap(0, std::sync::atomic::Ordering::Relaxed))
}

/// Submissions' askings begun ([`ask_once`]).
static SUBMISSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Submissions' askings between looks at the parked blocks that are no
/// placeholder (`VICTORY_PARK_EPOCH`, default 32; 1 looks at every one).
fn park_epoch() -> u64 {
    static N: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("VICTORY_PARK_EPOCH")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(32)
    })
}

/// The view blocks parked as uncommitted now ([`Guards::parked`]), and
/// those of them that are no placeholder.
pub(crate) fn parked_blocks() -> (usize, usize) {
    let g = guards();
    (g.parked.len(), g.parked_ordinary.len())
}

/// Parks `block`, found uncommitted and not clean ([`Guards::parked`]). A
/// block in a placeholder is not looked at again until the runtime maps
/// it: a plain commit into a placeholder fails, only a call that replaces
/// one fills it, and on title memory the runtime's are the only such calls
/// and tell of themselves ([`title_memory_remapped`]).
fn park(g: &mut Guards, block: usize) {
    g.unclean.remove(&block);
    g.parked.insert(block);
    if !in_placeholder(block) {
        g.parked_ordinary.insert(block);
    }
}

/// Whether `block` lies in a placeholder.
fn in_placeholder(block: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        MemoryRegionInfo, QueryVirtualMemoryInformation, WIN32_MEMORY_REGION_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // The region's flag for a placeholder reservation, after Private,
    // MappedDataFile, MappedImage, MappedPageFile, MappedPhysical,
    // DirectMapped, SoftwareEnclave and PageSize64K: the SDK's header calls
    // these bits reserved, the kernel fills them (Windows 10 1803 on). A
    // fresh, split or unmapped-to placeholder shows it, an ordinary
    // reservation (even one with no access) does not; `VirtualQuery` tells
    // the two apart by nothing.
    const PLACEHOLDER: u32 = 1 << 8;
    // SAFETY: plain data filled in by the query.
    let mut info: WIN32_MEMORY_REGION_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer of the right size; the returned size is
    // optional. Free memory fails the query: no placeholder.
    let ok = unsafe {
        QueryVirtualMemoryInformation(
            GetCurrentProcess(),
            (block * BLOCK) as *const _,
            MemoryRegionInfo,
            (&raw mut info).cast(),
            size_of_val(&info),
            std::ptr::null_mut(),
        )
    } != 0;
    // SAFETY: the flags word of the union, filled in by the query.
    ok && unsafe { info.Anonymous.Flags } & PLACEHOLDER != 0
}

/// Once per [`park_epoch`] submissions: the parked blocks that are no
/// placeholder and were committed since go back to the unclean set, no
/// longer absent, for the next asking that covers them to find written.
/// One question to Windows per uncommitted region.
fn unpark_committed(g: &mut Guards) {
    use std::sync::atomic::Ordering::Relaxed;
    let epoch = SUBMISSIONS.load(Relaxed) / park_epoch();
    if g.parked_at == epoch {
        return;
    }
    g.parked_at = epoch;
    let parked: Vec<usize> = g.parked_ordinary.iter().copied().collect();
    let mut until = 0;
    for block in parked {
        if block >= until {
            VIEW_STATS[4].fetch_add(1, Relaxed);
            // The block itself if it is (partly) committed now.
            until = uncommitted_end(block);
        }
        if block >= until {
            VIEW_STATS[5].fetch_add(1, Relaxed);
            g.parked_ordinary.remove(&block);
            g.parked.remove(&block);
            g.absent.remove(&block);
            g.unclean.insert(block);
        }
    }
}

/// Whether the block `page` is read-only by our hand and unwritten since a
/// copy took it (diagnostics of `VICTORY_BACK_OWN_CHECK`).
pub(crate) fn clean(page: usize) -> bool {
    guards_shared().clean.contains(page)
}

/// For view memory: the pages of `[start, end)`'s blocks written since
/// they were last guarded (all of a block's pages), guarding them again
/// before the caller copies them. `None` if the memory is not a view
/// (`known`: [`known_view`] said it is one).
fn guard(start: usize, end: usize, known: bool) -> Option<Vec<usize>> {
    #[cfg(feature = "diagnostics")]
    {
        use crate::copystat::{C, add};
        add(C::GuardAsks, 1);
        add(C::GuardKnown, u64::from(known));
    }
    if !known && !is_view(start, end) {
        return None;
    }
    install();
    let mut g = guards();
    apply_changed(&mut g);
    holes_due(&mut g);
    views_due(&mut g);
    let (first, last) = (start / BLOCK, end.div_ceil(BLOCK));
    if view_set() {
        return Some(guard_unclean(&mut g, first, last));
    }
    let mut written = Vec::new();
    let mut run: Option<usize> = None;
    // Nearly all of what is asked about is clean (a frame asks about some
    // 9 GB of view memory, 140,000 blocks): answered a word of blocks at
    // a time.
    if g.clean.holds(first..last) {
        // Nothing written, mapped or new since the range was last taken.
        #[cfg(feature = "diagnostics")]
        crate::copystat::add(crate::copystat::C::GuardClean, 1);
        return Some(written);
    }
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(crate::copystat::C::GuardBlocks, (last - first) as u64);
    // The range's clean blocks, in order: read off the set in one pass.
    // What the loop adds to the set meanwhile is the block it stands at
    // or ones before it.
    let clean: Vec<usize> = g.clean.range(first..last).collect();
    let mut next_clean = 0;
    let mut uncommitted_until = first;
    for block in first..=last {
        while next_clean < clean.len() && clean[next_clean] < block {
            next_clean += 1;
        }
        let is_clean = next_clean < clean.len() && clean[next_clean] == block;
        let mut open = block < last && !is_clean;
        if open && g.absent.contains(block) {
            // Ordinary commits need no runtime mapping notification. Reuse
            // this asking's query for the following blocks in the same region.
            if block >= uncommitted_until {
                uncommitted_until = hole_end(&mut g, block);
            }
            open = block >= uncommitted_until;
            if open {
                // Private memory now is never guarded. (From PR 907's fix in
                // myokex/victory-amd; asked only where a hole ended.)
                if is_view(block * BLOCK, (block + 1) * BLOCK) {
                    g.absent.remove(block);
                } else if block == first {
                    g.absent.remove(block);
                    return None;
                } else {
                    // It stays noted, so that the next asking finds it out
                    // again, and counts as written, since nothing watches it.
                    written.extend(block * BLOCK / PAGE..(block + 1) * BLOCK / PAGE);
                    open = false;
                }
                if holes_level() == 2 {
                    untold(block);
                }
            } else if holes_level() == 2 {
                // A hole still: it holds until it is due again, or told
                // mapped or committed.
                g.clean.insert(block);
            }
        }
        if open {
            written.extend(block * BLOCK / PAGE..(block + 1) * BLOCK / PAGE);
            if g.protected.contains(&block) {
                // Written through the runtime, or through another view of
                // its pages (`open`), still read-only. That view may be one
                // no copy checks: guarded again here, with the others.
                g.clean.insert(block);
                if view_guard() {
                    protect_aliases(&mut g, block);
                }
            } else {
                run.get_or_insert(block);
                continue;
            }
        }
        if let Some(from) = run.take() {
            protect(&mut g, from, block);
        }
    }
    Some(written)
}

/// [`guard`] with [`view_set`]: the same answer, from the view blocks that
/// are not clean ([`Guards::unclean`]) instead of every block of the range.
/// A range seen for the first time is noted as views, its blocks not clean
/// yet put in the set (once). Then: none in the range, nothing to do; else
/// each, in order, as the walk over the whole range treats it (one found
/// uncommitted parked; one written through the runtime, or guarded with
/// another view of its page, but still read-only noted clean; the others
/// guarded again in runs).
fn guard_unclean(g: &mut Guards, first: usize, last: usize) -> Vec<usize> {
    use std::sync::atomic::Ordering::Relaxed;
    unpark_committed(g);
    if !covered(&g.views, first, last) {
        let fresh: Vec<usize> = (first..last)
            .filter(|b| !g.clean.contains(b) && !covered(&g.views, *b, *b + 1))
            .collect();
        g.unclean.extend(fresh);
        add_range(&mut g.views, first, last);
    }
    let unclean: Vec<usize> = g.unclean.range(first..last).copied().collect();
    let mut written = Vec::new();
    if unclean.is_empty() {
        VIEW_STATS[0].fetch_add(1, Relaxed);
        return written;
    }
    VIEW_STATS[1].fetch_add(1, Relaxed);
    VIEW_STATS[2].fetch_add(unclean.len() as u64, Relaxed);
    let mut run: Option<(usize, usize)> = None;
    let flush = |g: &mut Guards, run: &mut Option<(usize, usize)>| {
        if let Some((a, b)) = run.take() {
            protect(g, a, b);
        }
    };
    for block in unclean {
        if g.absent.contains(&block) {
            // Found uncommitted before and still in the set (`protect`
            // parks the blocks it finds so): parked while it stays so.
            VIEW_STATS[4].fetch_add(1, Relaxed);
            if block < uncommitted_end(block) {
                park(g, block);
                flush(g, &mut run);
                continue;
            }
            g.absent.remove(&block);
        }
        written.extend(block * BLOCK / PAGE..(block + 1) * BLOCK / PAGE);
        if g.protected.contains(&block) {
            // Written through the runtime, or guarded with another view of
            // its page: still read-only. That view may be one no copy checks:
            // guarded again here, with the others.
            g.clean.insert(block);
            g.unclean.remove(&block);
            if view_guard() {
                protect_aliases(g, block);
            }
            flush(g, &mut run);
            continue;
        }
        run = match run {
            Some((a, b)) if b == block => Some((a, block + 1)),
            other => {
                if let Some((a, b)) = other {
                    protect(g, a, b);
                }
                Some((block, block + 1))
            }
        };
    }
    flush(g, &mut run);
    written
}

/// Whether `[start, end)` is view memory. The title's views lie in
/// placeholders, mapped and unmapped block by block as it streams, so a
/// range can begin in a block that is not mapped right now: the first
/// pages of the range that are there decide. A range with nothing there
/// counts as a view (its blocks are noted absent, and looked at again when
/// the runtime maps them).
///
/// Looking at the first address only took such a range for "not a view":
/// its copy was marked not watched for good and copied whole at every
/// refresh, around its unmapped pages (run 54: 300 such ranges, 570 MB
/// copied a frame for them, 255 copies a frame reading around holes).
/// `VICTORY_GUARD_HOLES=0` looks at the first address only, as before.
fn is_view(start: usize, end: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_MAPPED, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    static HOLES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let holes = *HOLES.get_or_init(|| std::env::var_os("VICTORY_GUARD_HOLES").is_none_or(|v| v != "0"));
    let mut at = start;
    loop {
        // SAFETY: plain data filled in by the query.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: a valid out pointer of the right size.
        let n = unsafe { VirtualQuery(at as *const _, &mut info, size_of_val(&info)) };
        #[cfg(feature = "diagnostics")]
        crate::copystat::add(crate::copystat::C::GuardQueries, 1);
        if n == 0 {
            return false;
        }
        if info.Type == MEM_MAPPED {
            return true;
        }
        // Committed memory that is no view (the title's heaps, ours), or
        // the answer as it was before.
        if info.State == MEM_COMMIT || !holes {
            return false;
        }
        // A hole (a placeholder, or free): on to what follows it.
        let next = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        if next <= at || next >= end {
            return true;
        }
        at = next;
    }
}

/// Makes blocks `[from, to)` read-only and notes them guarded and clean;
/// block by block if they span views, leaving out any that fail, and
/// noting those not committed (unmapped) absent. The same physical pages
/// at every other view of them are made read-only too: a write through
/// one of those is this memory written, and only a fault can say so.
/// `VICTORY_VIEW_GUARD=0` leaves the other views as they are, as before.
fn protect(g: &mut Guards, from: usize, to: usize) {
    protect_here(g, from, to);
    if !view_guard() {
        return;
    }
    // Single `inplace::VIEWS` lock across `[from, to)` + coalesced `VirtualProtect`
    // runs across contiguous alias blocks (avoids thousands of per-block locks/syscalls).
    let (all_aliased, others, blocks_counted) = crate::inplace::alias_blocks_in(from, to, MANY_VIEWS);
    if blocks_counted == 0 {
        return;
    }
    #[cfg(feature = "diagnostics")]
    ALIASES[0].fetch_add(blocks_counted, std::sync::atomic::Ordering::Relaxed);
    g.aliased.extend(all_aliased);
    let mut run: Option<(usize, usize)> = None;
    for alias in others {
        if g.protected.contains(&alias) {
            continue;
        }
        match run {
            Some((a, b)) if alias == b => run = Some((a, alias + 1)),
            Some((a, b)) => {
                protect_alias_run(g, a, b);
                run = Some((alias, alias + 1));
            }
            None => run = Some((alias, alias + 1)),
        }
    }
    if let Some((a, b)) = run {
        protect_alias_run(g, a, b);
    }
}

/// Coalesced `protect_alias` for contiguous alias blocks `[from, to)`: one
/// `VirtualProtect` syscall when the run succeeds, per-block fallback otherwise.
fn protect_alias_run(g: &mut Guards, from: usize, to: usize) {
    use windows_sys::Win32::System::Memory::{PAGE_READONLY, VirtualProtect};
    if to <= from {
        return;
    }
    let mut old = 0;
    // SAFETY: title view memory; only its protection changes.
    if unsafe { VirtualProtect((from * BLOCK) as *const _, (to - from) * BLOCK, PAGE_READONLY, &mut old) } != 0 {
        g.protected.extend(from..to);
        return;
    }
    for block in from..to {
        protect_alias(g, block);
    }
}

/// Makes the other views of `block`'s pages read-only and notes them all
/// guarded together, `block` included (see [`protect`]).
fn protect_aliases(g: &mut Guards, block: usize) {
    let shown = crate::inplace::pages_of(block * BLOCK);
    // One view (nothing else to guard), or the page the title shows
    // wherever it has nothing (thousands of views): each view of that one
    // is guarded by itself.
    if shown.len() < 2 || shown.len() > MANY_VIEWS {
        return;
    }
    #[cfg(feature = "diagnostics")]
    ALIASES[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for alias in shown {
        let alias = alias / BLOCK;
        g.aliased.insert(alias);
        if alias != block {
            protect_alias(g, alias);
        }
    }
}

/// Views of one page past which its views are not guarded together.
pub(crate) const MANY_VIEWS: usize = 64;

/// Whether all the views of a guarded page are guarded together
/// (`VICTORY_VIEW_GUARD`).
fn view_guard() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_VIEW_GUARD").is_none_or(|v| v != "0"))
}

/// Diagnostics: blocks guarded together with their other views, blocks
/// noted written because another view of theirs was written, and blocks
/// opened because a view of theirs was mapped; since the start.
#[cfg(feature = "diagnostics")]
pub(crate) static ALIASES: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

/// Makes `block` read-only: another view of pages a copy has just taken
/// through its own view, so that a write through this one faults too. It is
/// not noted clean. The copies of title memory at this view have not taken
/// the pages: noted clean, the block would not be reported written at their
/// next check and they would keep the bytes from before the write (`guard`
/// reports a block that is read-only and not clean as written, then notes it
/// clean). A block that is clean stays so: had the pages been written since
/// its copies took them, `open` would have noted it written.
fn protect_alias(g: &mut Guards, block: usize) {
    use windows_sys::Win32::System::Memory::{PAGE_READONLY, VirtualProtect};
    if g.protected.contains(&block) {
        return;
    }
    let mut old = 0;
    // SAFETY: title view memory; only its protection changes.
    if unsafe { VirtualProtect((block * BLOCK) as *const _, BLOCK, PAGE_READONLY, &mut old) } != 0 {
        g.protected.insert(block);
    }
}

/// [`protect`] for one range, without the other views of its pages.
fn protect_here(g: &mut Guards, from: usize, to: usize) {
    use windows_sys::Win32::System::Memory::{PAGE_READONLY, VirtualProtect};
    let set = |from: usize, to: usize| {
        #[cfg(feature = "diagnostics")]
        crate::copystat::add(crate::copystat::C::GuardProtects, 1);
        let mut old = 0;
        // SAFETY: title view memory; only its protection changes.
        unsafe {
            VirtualProtect(
                (from * BLOCK) as *const _,
                (to - from) * BLOCK,
                PAGE_READONLY,
                &mut old,
            ) != 0
        }
    };
    let note = |g: &mut Guards, from: usize, to: usize| {
        g.protected.extend(from..to);
        g.clean.extend(from..to);
        for block in from..to {
            g.unclean.remove(&block);
        }
    };
    if set(from, to) {
        note(g, from, to);
        return;
    }
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(crate::copystat::C::GuardProtectFails, 1);
    // Block by block; a block that is not committed answers for the whole
    // uncommitted run it begins (one question, no protection asked for the
    // rest: 77 runs a frame were refused whole in the world, with 10,264
    // questions to Windows and 1,120 protections a frame, run int-i-1).
    let mut block = from;
    while block < to {
        if set(block, block + 1) {
            note(g, block, block + 1);
            block += 1;
            continue;
        }
        #[cfg(feature = "diagnostics")]
        let error = std::io::Error::last_os_error();
        let end = uncommitted_end(block).min(to);
        if end > block {
            g.absent.extend(block..end);
            // Out of the unclean set while they stay so ([`Guards::parked`]).
            for absent in block..end {
                if g.unclean.contains(&absent) {
                    park(g, absent);
                }
            }
            if holes_level() == 2 {
                // Nothing to copy there until it is told mapped or committed:
                // the asking's first look takes the range as clean.
                g.clean.extend(block..end);
            }
            block = end;
        } else {
            // Diagnostics: a committed block that will not be made
            // read-only is reported written at every asking and never
            // known to the guards (one in 1,024 said).
            #[cfg(feature = "diagnostics")]
            {
                static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 1024 == 0 {
                    tracing::info!("a block of title memory could not be made read-only (one in 1024): {:#x}: {error}", block * BLOCK);
                }
            }
            block += 1;
        }
    }
}

/// The end (exclusive) of the whole uncommitted blocks starting at `block`.
fn uncommitted_end(block: usize) -> usize {
    use windows_sys::Win32::System::Memory::{
        MEM_FREE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    // SAFETY: plain data filled in by the query.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer of the right size.
    let n = unsafe { VirtualQuery((block * BLOCK) as *const _, &mut info, size_of_val(&info)) };
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(crate::copystat::C::GuardQueries, 1);
    if n != 0 && matches!(info.State, MEM_RESERVE | MEM_FREE) {
        (info.BaseAddress as usize).saturating_add(info.RegionSize) / BLOCK
    } else {
        block
    }
}

/// How long what Windows said of a run of uncommitted blocks holds
/// (`VICTORY_GUARD_HOLES_FRAME`): 0 (unset) every asking questions Windows
/// again; 1 for the rest of the frame; 2 until the runtime tells of a
/// mapping or a commit there.
///
/// The title's streamed buffers lie over placeholders with holes, and a
/// submission's askings walk them: 10,264 questions a frame in the world,
/// 7.5 ms of the submitting thread (run int-i-1), nearly all with the
/// answer of the asking before; with the title's small buffers on the card
/// they were the largest single cost of that thread (8.1 ms a frame of
/// `NtQueryVirtualMemory`, run int-m-kb).
///
/// shortcut (1): a block the title commits by an ordinary commit (no word
/// from the runtime) after the frame's first question about it is copied
/// as zeros until the next frame begins.
///
/// shortcut (2): a hole found holds, with no question and no walk of its
/// blocks (they count as clean), until the runtime tells of a mapping
/// ([`title_memory_remapped`]) or of a commit ([`title_pages_changed`]: the
/// title's `VirtualAlloc` and `XMemVirtualAlloc`, and our own allocations
/// for it), or until it is due a look again ([`holes_due`]). Memory
/// committed in a way the runtime does not tell of is copied as zeros
/// until then, and is logged when the look finds it. In the world: 443
/// questions a frame, none of the holes looked at again found committed
/// (run st-q-kv2), against 12,210 questions a frame (run st-o-kv).
fn holes_level() -> u8 {
    static LEVEL: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("VICTORY_GUARD_HOLES_FRAME").ok().and_then(|v| v.trim().parse().ok()).filter(|&v| v <= 2).unwrap_or(0)
    })
}

/// Ranges told by [`title_pages_changed`] and not applied yet: the next
/// asking takes them, so the title's threads never wait for the guards.
static CHANGED: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static CHANGED_ANY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the runtime should tell of ordinary commits and frees
/// ([`holes_level`] 2).
pub fn title_pages_told() -> bool {
    holes_level() == 2
}

/// The title's memory `[at, at + len)` was committed, decommitted or
/// released by an ordinary call (no mapping changed): with
/// [`holes_level`] 2, the blocks noted absent there are looked at again.
/// Nothing to do otherwise.
pub fn title_pages_changed(at: usize, len: usize) {
    if !title_pages_told() || len == 0 {
        return;
    }
    CHANGED.lock().unwrap_or_else(|e| e.into_inner()).push((at, len));
    CHANGED_ANY.store(true, std::sync::atomic::Ordering::Release);
}

/// Applies what [`title_pages_changed`] was told since the last asking.
fn apply_changed(g: &mut Guards) {
    if !CHANGED_ANY.swap(false, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let changed = std::mem::take(&mut *CHANGED.lock().unwrap_or_else(|e| e.into_inner()));
    for (at, len) in changed {
        let absent: Vec<usize> = g.absent.range(at / BLOCK..at.saturating_add(len).div_ceil(BLOCK)).collect();
        for block in absent {
            g.absent.remove(block);
            g.clean.remove(block);
        }
    }
}

/// With [`holes_level`] 2: every how many frames a hole is looked at again
/// (`VICTORY_GUARD_HOLES_RECHECK`, 16 unset; 0 never).
fn holes_recheck() -> usize {
    static EVERY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| {
        std::env::var("VICTORY_GUARD_HOLES_RECHECK").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(16)
    })
}

/// With [`holes_level`] 2, once a frame: a share of the holes (their words
/// of blocks by number, one share in [`holes_recheck`] each frame) is no
/// longer clean, so the askings of this frame and later question Windows
/// about them again, each once.
fn holes_due(g: &mut Guards) {
    if holes_level() != 2 {
        return;
    }
    let every = holes_recheck();
    let frame = crate::queue::presented();
    if every == 0 || g.due_frame == frame {
        return;
    }
    g.due_frame = frame;
    let turn = frame as usize % every;
    let Guards { absent, clean, .. } = g;
    for (&word, &bits) in &absent.0 {
        if word % every == turn {
            clean.clear(word, bits);
        }
    }
}

/// Every how many frames a guarded block is looked at again though no
/// write to it was seen (`VICTORY_VIEW_RECHECK`, in frames; unset or 0:
/// never).
///
/// A write through another view of a block's pages does not fault at the
/// guarded one. [`view_guard`] has every view guarded so that it does,
/// which cost 30 ms a frame in the world (run st-u-vg: 96-102 ms a frame
/// where st-u-wc without it made 68-73). Without it the copy at the
/// guarded view keeps its bytes for good. This bounds for how long: each
/// block is reported written once in so many frames, and its copies read
/// it again (with `VICTORY_REFRESH_COMPARE=1` they take only what
/// differs; without it they take the block whole).
fn views_recheck() -> usize {
    static EVERY: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| std::env::var("VICTORY_VIEW_RECHECK").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0))
}

/// Diagnostics: clean blocks made due by [`views_due`] since the last
/// frame-rate line.
#[cfg(feature = "diagnostics")]
pub(crate) static DUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// With [`views_recheck`], once a frame: a share of the clean guarded
/// blocks is no longer clean, so the next asking about them reports them
/// written. They stay read-only: `guard` notes them clean again with no
/// call to Windows.
fn views_due(g: &mut Guards) {
    let every = views_recheck();
    let frame = crate::queue::presented();
    if every == 0 || g.views_frame == frame {
        return;
    }
    g.views_frame = frame;
    for (word, bits) in due_bits(&g.clean, &g.absent, every, frame as usize % every) {
        #[cfg(feature = "diagnostics")]
        DUE.fetch_add(u64::from(bits.count_ones()), std::sync::atomic::Ordering::Relaxed);
        g.clean.clear(word, bits);
    }
}

/// The share of `clean` that [`views_due`] makes due at `turn` of `every`:
/// the words of blocks whose number is `turn` modulo `every`, with their
/// clean bits, without the holes (`absent`: those are [`holes_due`]'s,
/// and making one due questions Windows).
fn due_bits(clean: &Blocks, absent: &Blocks, every: usize, turn: usize) -> Vec<(usize, u64)> {
    clean
        .0
        .iter()
        .filter(|&(&word, _)| word % every == turn)
        .map(|(&word, &bits)| (word, bits & !absent.0.get(&word).copied().unwrap_or(0)))
        .filter(|&(_, bits)| bits != 0)
        .collect()
}

/// A hole that was due a look is committed or mapped, and the runtime had
/// not told ([`holes_level`] 2): counted, the first ones named with what
/// Windows says of them (which way of committing the runtime misses).
fn untold(block: usize) {
    use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seen = SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if seen > 48 && !seen.is_power_of_two() {
        return;
    }
    // SAFETY: plain data filled in by the query.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer of the right size.
    unsafe { VirtualQuery((block * BLOCK) as *const _, &mut info, size_of_val(&info)) };
    tracing::warn!(
        address = format_args!("{:#x}", block * BLOCK),
        state = format_args!("{:#x}", info.State),
        kind = format_args!("{:#x}", info.Type),
        protect = format_args!("{:#x}", info.Protect),
        allocation = format_args!("{:p}", info.AllocationBase),
        region = info.RegionSize,
        seen,
        "a hole was committed or mapped with no word from the runtime"
    );
}

/// [`uncommitted_end`] of an absent `block`, from this frame's answers
/// with [`holes_level`] 1 and 2.
fn hole_end(g: &mut Guards, block: usize) -> usize {
    if holes_level() == 0 {
        return uncommitted_end(block);
    }
    let frame = crate::queue::presented();
    if g.holes_frame != frame {
        g.holes.clear();
        g.holes_frame = frame;
    }
    if let Some((_, &end)) = g.holes.range(..=block).next_back()
        && block < end
    {
        return end;
    }
    let end = uncommitted_end(block);
    if end > block {
        g.holes.insert(block, end);
    }
    end
}

/// Gives write access back to the guarded blocks of `[start, end)` and
/// notes them written; true if any was guarded. The same physical pages at
/// every other view of them are noted written too: the write that faulted
/// (or is ours, `crate::host::copy_mapped`) goes through one view, and
/// every guard over the pages must copy them again.
///
/// Those other views stay read-only. `guard` reports a block that is
/// read-only and not clean as written, and a write through one of them
/// still faults and opens it. Opening them all cost two changes of
/// protection for every other view of every page written (one here, one
/// when the next check guarded it again), most of them for views the
/// title did not write through before that check.
fn open(g: &mut Guards, start: usize, end: usize) -> bool {
    let any = open_here(g, start, end);
    if start >= end || g.aliased.is_empty() {
        return any;
    }
    let aliased: Vec<usize> = g.aliased.range(start / BLOCK..end.div_ceil(BLOCK)).copied().collect();
    for block in aliased {
        // Guarded with the others again when its own check, or the next
        // check of one of them, finds the pages written.
        g.aliased.remove(&block);
        for alias in crate::inplace::pages_of(block * BLOCK) {
            let alias = alias / BLOCK;
            if alias != block && g.protected.contains(&alias) && g.clean.remove(&alias) {
                if covered(&g.views, alias, alias + 1) {
                    g.unclean.insert(alias);
                }
                #[cfg(feature = "diagnostics")]
                ALIASES[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    any
}

/// [`open`] for one range, without the other views of its pages.
fn open_here(g: &mut Guards, start: usize, end: usize) -> bool {
    use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};
    let blocks: Vec<usize> = g
        .protected
        .range(start / BLOCK..end.div_ceil(BLOCK))
        .copied()
        .collect();
    for &block in &blocks {
        g.protected.remove(&block);
        g.clean.remove(block);
    }
    // Neighbours in one call (each call tells every processor the title
    // runs on); one by one if the call fails, as over more than one view
    // (after reddxd-byte/ShakiraRED-dt c2271e5).
    let set = |from: usize, to: usize| {
        let mut old = 0;
        // SAFETY: blocks we made read-only.
        unsafe { VirtualProtect((from * BLOCK) as *const _, (to - from) * BLOCK, PAGE_READWRITE, &mut old) != 0 }
    };
    for run in blocks.chunk_by(|a, b| a + 1 == *b) {
        let (from, to) = (run[0], run[run.len() - 1] + 1);
        if !set(from, to) && run.len() > 1 {
            for block in from..to {
                set(block, block + 1);
            }
        }
        // A view block: not clean ([`Guards::unclean`]).
        for block in from..to {
            if covered(&g.views, block, block + 1) {
                g.unclean.insert(block);
            }
        }
    }
    !blocks.is_empty()
}

/// Before our own write into title memory `[at, at + len)` through the
/// kernel (which does not fault into our handler): opens its guarded blocks.
/// The runtime calls it (as `kernel_will_write`) before the title's kernel
/// reads into its own memory.
/// After the layer wrote `[at, at + len)` of title memory through the
/// kernel: the dirty bits the kernel holds for those pages are the layer's
/// own writes, and a refresh that took them for the title's copies the same
/// bytes up again every frame. Asks the watch to clear them and drops the
/// answer (nothing is written into title memory between the write and this
/// asking by design: both are the layer's).
pub(crate) fn forgot(at: usize, len: usize) {
    use windows_sys::Win32::System::Memory::GetWriteWatch;
    const RESET: u32 = 1;
    if len == 0 {
        return;
    }
    let mut buf = [0usize; 512];
    let first = at / KERNEL_PAGE * KERNEL_PAGE;
    let end = at.saturating_add(len);
    let mut page = first;
    while page < end {
        let pages = (end - page).div_ceil(KERNEL_PAGE).min(buf.len());
        let span = pages * KERNEL_PAGE;
        let mut count = pages;
        let mut granularity = 0u32;
        // SAFETY: `count` entries in the buffer, a whole number of kernel
        // pages of our own process's memory.
        let _ = unsafe {
            GetWriteWatch(
                RESET,
                page as *const _,
                span,
                buf.as_mut_ptr().cast(),
                &mut count,
                &mut granularity,
            )
        };
        page += span;
    }
}

pub fn writing(at: usize, len: usize) {
    // Copies checked in this submission check again (`crate::stamp`).
    crate::stamp::advance();
    // The length is the title's (`fread`'s size times count, saturated).
    let end = at.saturating_add(len);
    // Nothing there is guarded, as for most of our own writes: nothing to
    // open, and nobody waits for the guards.
    let blocks = at / BLOCK..end.div_ceil(BLOCK);
    let any = at < end && {
        let g = guards_shared();
        g.protected.range(blocks.clone()).next().is_some() || g.aliased.range(blocks).next().is_some()
    };
    if any {
        open(&mut guards(), at, end);
    }
    // A placed texture there changes with no descriptor written.
    crate::descriptor::host::remapped(at, len);
}

/// What `VICTORY_PAGE_WRITES` asks to be done with a write the guards do
/// not see once its bytes are there ([`title_memory_written`]), as bits:
/// 1, the guarded blocks under it are opened and count as written (host
/// copies of buffers and textures read through tables take them again);
/// 2, the placed textures picked by index that lie under it are due again;
/// 4, both also for reads to plain addresses when they have finished.
/// Unset or `guards`: bit 1 alone. `0`: nothing, counted only (the default
/// until run 199). `1`: bits 1 and 2. `slots`: bit 2 alone. `all`:
/// everything.
/// (Bit 1 by default: in every run 10,000 to 50,000 guarded blocks of host
/// copies lay under such writes and kept the bytes from before; runs 197
/// and 198 opened them, 9,766 and 10,796 blocks, with nothing worse for
/// it. Bit 2 makes texture slots due and is the one that flooded the
/// queue in an earlier form: still asked for by name.)
pub(crate) fn page_writes() -> u8 {
    static MODE: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("VICTORY_PAGE_WRITES").as_deref().map(str::trim) {
        Ok("0") => 0,
        Ok("1") => 3,
        Ok("slots") => 2,
        Ok("all") => 7,
        _ => 1,
    })
}

/// Since the start: writes into physical pages through the runtime's own
/// views that landed at addresses the title has mapped (one for each run
/// of addresses); their bytes; guarded blocks that lay under them (host
/// copies that would otherwise keep the bytes from before); reads to
/// plain addresses that finished.
pub(crate) static WRITTEN: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];

/// The bytes of a write the guards do not see are there: title memory
/// `[at, at + len)`. `pages`: DirectStorage wrote them into physical pages
/// through a view of the runtime's own, and this is an address where the
/// title has those pages mapped (no title address faulted, nothing was
/// mapped or unmapped; the shortcut this file's header names). Otherwise a
/// read to a plain address has finished, of which [`writing`] was told
/// before it began.
///
/// Upstream's c75fdba does the same for its trackers (its `Writing` marks
/// the pages once more when dropped; page destinations are reported at
/// each address showing them). Here it is counted always and acted on as
/// [`page_writes`] says. Called without the runtime's page-table lock;
/// takes the guards' lock, then, alone, the lock of the ranges kept for
/// the texture pass.
pub fn title_memory_written(at: usize, len: usize, pages: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    if len == 0 {
        return;
    }
    crate::stamp::advance();
    let mode = page_writes();
    let end = at.saturating_add(len);
    if pages {
        WRITTEN[0].fetch_add(1, Relaxed);
        WRITTEN[1].fetch_add(len as u64, Relaxed);
    } else {
        WRITTEN[3].fetch_add(1, Relaxed);
        if mode & 4 == 0 {
            return;
        }
    }
    let guarded = guards_shared().protected.range(at / BLOCK..end.div_ceil(BLOCK)).count() as u64;
    if pages {
        WRITTEN[2].fetch_add(guarded, Relaxed);
    }
    if mode & 1 != 0 && guarded != 0 {
        open(&mut guards(), at, end);
    }
    // A placed texture there changed with no descriptor written.
    crate::descriptor::host::page_written(at, len);
}

/// The runtime mapped a view of physical pages (`crate::inplace::title_memory_view`):
/// the new view is writable, so the same bytes guarded at another view must be
/// checked again — and are, the pages noted written, which also has the next
/// check guard the new view (`protect`).
pub(crate) fn views_mapped(ranges: &[(usize, usize)]) {
    if !view_guard() || ranges.is_empty() {
        return;
    }
    let mut g = guards();
    for &(at, n) in ranges {
        let end = at.saturating_add(n);
        let aliased: Vec<usize> = g.aliased.range(at / BLOCK..end.div_ceil(BLOCK)).copied().collect();
        for block in aliased {
            g.aliased.remove(&block);
        }
        if open_here(&mut g, at, end) {
            #[cfg(feature = "diagnostics")]
            ALIASES[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Held shared by a read of title memory in place, and alone by the runtime
/// while it changes what title memory shows.
///
/// The runtime does not change a mapping in one step. To unmap one page of
/// a view it unmaps the whole view and maps what stays again
/// (`xmem.rs` `unmap`), and a view it maps is a reservation until the next
/// call commits it (`map`, then `commit`). For that moment pages the title
/// never gave up are "reserved, never committed", and a thread reading
/// them in place takes an access violation: run 71 in `Layout::detile`,
/// run 74 in the hash before it, both under `Staging::refresh_now`, which
/// had asked whether the texture was all mapped and been told yes. Asking
/// cannot help; not changing the mapping while such a read is under way
/// does.
///
/// Only with `VICTORY_MAPPING_RANGES=0` ([`by_range`]); otherwise reads and
/// changes hold the bytes they touch ([`SPANS`]).
static MAPPING: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Whether a read in place and a change of mapping keep apart only when
/// their bytes meet ([`SPANS`]), not whatever memory each touches
/// ([`MAPPING`]); `VICTORY_MAPPING_RANGES=0` holds all of title memory for
/// each, as before. On by default.
///
/// With one lock over all of title memory each texture refresh waited for
/// every change of mapping anywhere, and each change for the refresh under
/// way. While the world streams in the title changes mappings 650-1,100
/// times a frame, many a page at a time (`XMemMapPhysicalPagesScatter`
/// maps each page on its own). With the one lock the refreshes waited 203
/// ms of an 801 ms frame for title memory to hold still; holding only the
/// bytes touched, 0 ms of a 486 ms frame (medians, a pair of runs).
pub(crate) fn by_range() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_MAPPING_RANGES").is_none_or(|v| v != "0"))
}

/// Reads of title memory in place under way, and changes of its mapping,
/// by the bytes they touch ([`by_range`]).
struct Spans {
    /// To tell them apart.
    next: u64,
    /// Reads under way: number, start and end.
    reads: Vec<(u64, usize, usize)>,
    /// Changes: number, start, end, and whether under way (else waiting for
    /// the reads over its bytes to end; no read begins over them meanwhile,
    /// so that one read after another cannot keep a change waiting).
    changes: Vec<(u64, usize, usize, bool)>,
    /// Threads waiting on [`SPANS_LEFT`].
    waiting: usize,
}

static SPANS: Mutex<Spans> = Mutex::new(Spans {
    next: 0,
    reads: Vec::new(),
    changes: Vec::new(),
    waiting: 0,
});

/// Told when a read or a change ends while a thread waits.
static SPANS_LEFT: std::sync::Condvar = std::sync::Condvar::new();

fn spans() -> std::sync::MutexGuard<'static, Spans> {
    SPANS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Waits until a read or a change ends.
fn spans_wait(mut s: std::sync::MutexGuard<'static, Spans>) -> std::sync::MutexGuard<'static, Spans> {
    s.waiting += 1;
    let mut s = SPANS_LEFT.wait(s).unwrap_or_else(|e| e.into_inner());
    s.waiting -= 1;
    s
}

/// Whether `[a, b)` and `[c, d)` share a byte.
fn meet(a: usize, b: usize, c: usize, d: usize) -> bool {
    a < d && c < b
}

impl Spans {
    fn read(&mut self, at: usize, end: usize) -> Span {
        self.next += 1;
        self.reads.push((self.next, at, end));
        Span(self.next, false)
    }
}

/// A read's or a change's place in [`SPANS`] (its number, and whether a
/// change), given up when dropped.
struct Span(u64, bool);

impl Drop for Span {
    fn drop(&mut self) {
        let mut s = spans();
        if self.1 {
            s.changes.retain(|c| c.0 != self.0);
        } else if let Some(i) = s.reads.iter().position(|r| r.0 == self.0) {
            s.reads.swap_remove(i);
        }
        if s.waiting > 0 {
            SPANS_LEFT.notify_all();
        }
    }
}

/// What a read or a change of mapping holds: its bytes ([`by_range`]) or
/// the lock over all of title memory.
enum Held<G> {
    All(#[allow(dead_code)] G),
    Bytes(#[allow(dead_code)] Span),
}

/// Since the last frame-rate line: changes of title memory's mapping, the
/// microseconds they waited for reads in place, and those they were under
/// way.
pub(crate) static REMAPS: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

/// What the runtime holds while it changes what title memory shows
/// ([`title_memory_remapping`]).
pub struct Remapping {
    _held: Held<std::sync::RwLockWriteGuard<'static, ()>>,
    asked: std::time::Instant,
    since: std::time::Instant,
}

impl Drop for Remapping {
    fn drop(&mut self) {
        REMAPS[2].fetch_add(self.since.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
        // From asking for the lock until it is let go (the frame log).
        crate::framelog::add(crate::framelog::T::RemapUs, self.asked.elapsed().as_micros() as u64);
    }
}

/// The runtime is about to map, unmap or move physical pages of title
/// memory `[at, at + len)`: until what it returns is dropped, no read of
/// those bytes in place is under way and none begins
/// ([`title_memory_reading`]). Taken around the whole change, the unmapping
/// and the mapping again, over all the bytes whose mapping it changes.
pub fn title_memory_remapping(at: usize, len: usize) -> Remapping {
    use std::sync::atomic::Ordering::Relaxed;
    let started = std::time::Instant::now();
    let held = if by_range() {
        let end = at.saturating_add(len);
        let mut s = spans();
        s.next += 1;
        let id = s.next;
        s.changes.push((id, at, end, false));
        while s.reads.iter().any(|r| meet(r.1, r.2, at, end))
            || s.changes.iter().any(|c| c.3 && meet(c.1, c.2, at, end))
        {
            s = spans_wait(s);
        }
        if let Some(c) = s.changes.iter_mut().find(|c| c.0 == id) {
            c.3 = true;
        }
        Held::Bytes(Span(id, true))
    } else {
        Held::All(MAPPING.write().unwrap_or_else(|e| e.into_inner()))
    };
    REMAPS[0].fetch_add(1, Relaxed);
    REMAPS[1].fetch_add(started.elapsed().as_micros() as u64, Relaxed);
    crate::framelog::add(crate::framelog::T::Remaps, 1);
    crate::framelog::add(crate::framelog::T::RemapWaitUs, started.elapsed().as_micros() as u64);
    Remapping {
        _held: held,
        asked: started,
        since: std::time::Instant::now(),
    }
}

/// What a read of title memory in place holds ([`title_memory_reading`]).
pub struct Reading(#[allow(dead_code)] Held<std::sync::RwLockReadGuard<'static, ()>>);

/// For reading title memory `[at, at + len)` in place: until what it
/// returns is dropped, the runtime changes no mapping of those bytes, so
/// memory found mapped stays mapped. Not to be taken twice by one thread,
/// nor held across anything that maps title memory.
pub fn title_memory_reading(at: usize, len: usize) -> Reading {
    if !by_range() {
        return Reading(Held::All(MAPPING.read().unwrap_or_else(|e| e.into_inner())));
    }
    let end = at.saturating_add(len);
    let mut s = spans();
    while s.changes.iter().any(|c| meet(c.1, c.2, at, end)) {
        s = spans_wait(s);
    }
    Reading(Held::Bytes(s.read(at, end)))
}

/// [`title_memory_reading`] if it can be had at once: `None` while the
/// runtime changes a mapping of those bytes (or waits to), for a caller
/// that has another way to read and would rather not wait.
pub fn title_memory_reading_now(at: usize, len: usize) -> Option<Reading> {
    if !by_range() {
        return match MAPPING.try_read() {
            Ok(guard) => Some(Reading(Held::All(guard))),
            Err(std::sync::TryLockError::Poisoned(e)) => Some(Reading(Held::All(e.into_inner()))),
            Err(std::sync::TryLockError::WouldBlock) => None,
        };
    }
    let end = at.saturating_add(len);
    let mut s = spans();
    if s.changes.iter().any(|c| meet(c.1, c.2, at, end)) {
        return None;
    }
    Some(Reading(Held::Bytes(s.read(at, end))))
}

/// The runtime mapped or unmapped title memory `[at, at + len)`: what it
/// shows is new and read-write, so it is no longer guarded (nor known
/// unmapped).
pub fn title_memory_remapped(at: usize, len: usize) {
    // Stored pages followed there end (`VICTORY_BACK_READS`): counted only.
    #[cfg(feature = "diagnostics")]
    crate::backreads::remapped(at, len);
    crate::texback::remapped(at, len);
    crate::stamp::advance();
    // A placed texture there arrives, goes or changes with no descriptor
    // written.
    crate::descriptor::host::remapped(at, len);
    // The views the runtime told us of there are no longer what it shows.
    let reopen = crate::inplace::views_gone(at, len);
    // And nothing stays in place over it.
    crate::inplace::title_memory_freeing(at, len);
    let mut g = guards();
    g.holes.clear();
    let range = at / BLOCK..at.saturating_add(len).div_ceil(BLOCK);
    let absent: Vec<usize> = g.absent.range(range.clone()).collect();
    for block in absent {
        g.absent.remove(block);
        g.clean.remove(block);
    }
    let blocks: Vec<usize> = g.protected.range(range.clone()).copied().collect();
    for block in blocks {
        g.protected.remove(&block);
        g.clean.remove(block);
    }
    let aliased: Vec<usize> = g.aliased.range(at / BLOCK..at.saturating_add(len).div_ceil(BLOCK)).copied().collect();
    for block in aliased {
        g.aliased.remove(&block);
    }
    for (addr, n) in reopen {
        let end = addr.saturating_add(n);
        let stale: Vec<usize> = g.aliased.range(addr / BLOCK..end.div_ceil(BLOCK)).copied().collect();
        for block in stale {
            g.aliased.remove(&block);
        }
        open_here(&mut g, addr, end);
    }
    // Nor parked: the views below go back to the unclean set.
    let parked: Vec<usize> = g.parked.range(range.clone()).copied().collect();
    for block in parked {
        g.parked.remove(&block);
        g.parked_ordinary.remove(&block);
    }
    // View blocks there are not clean any more.
    let from = g.views.partition_point(|r| r.1 <= range.start);
    let views: Vec<usize> = g.views[from..]
        .iter()
        .take_while(|r| r.0 < range.end)
        .flat_map(|&(a, b)| a.max(range.start)..b.min(range.end))
        .collect();
    g.unclean.extend(views);
}

/// The title committed `[at, at + len)` through the runtime
/// (`XMemVirtualAlloc`): blocks of it found uncommitted before are not
/// known so any more, and the next asking that covers them finds them
/// written. A commit no one tells of is found at the next look at the
/// parked blocks that are no placeholder ([`Guards::parked`]).
pub fn title_memory_committed(at: usize, len: usize) {
    let mut g = guards();
    let range = at / BLOCK..at.saturating_add(len).div_ceil(BLOCK);
    let parked: Vec<usize> = g.parked.range(range.clone()).copied().collect();
    for block in parked {
        g.parked.remove(&block);
        g.parked_ordinary.remove(&block);
        g.unclean.insert(block);
    }
    let absent: Vec<usize> = g.absent.range(range).copied().collect();
    for block in absent {
        g.absent.remove(&block);
    }
}

/// Registers the handler that takes writes to guarded blocks, once, ahead
/// of the crash reporter's.
fn install() {
    use windows_sys::Win32::Foundation::{EXCEPTION_ACCESS_VIOLATION, NTSTATUS};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
        EXCEPTION_POINTERS,
    };
    unsafe extern "system" fn on_write(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes the exception being dispatched.
        let r = unsafe { &*(*info).ExceptionRecord };
        const WRITE: usize = 1;
        if r.ExceptionCode != EXCEPTION_ACCESS_VIOLATION as NTSTATUS
            || r.NumberParameters < 2
            || r.ExceptionInformation[0] != WRITE
        {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let at = r.ExceptionInformation[1];
        // Held across both checks: another thread may have opened the block
        // since this one faulted, and a copy may have guarded it again since
        // (then it is ours again, not read-only for some other reason).
        let mut g = guards();
        if open(&mut g, at, at + 1) {
            #[cfg(feature = "diagnostics")]
            crate::copystat::add(crate::copystat::C::GuardFaults, 1);
            crate::framelog::add(crate::framelog::T::GuardFaults, 1);
            return EXCEPTION_CONTINUE_EXECUTION;
        }
        if writable(at) {
            EXCEPTION_CONTINUE_EXECUTION
        } else {
            EXCEPTION_CONTINUE_SEARCH
        }
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: the handler only touches our guard state and protections.
    ONCE.call_once(|| unsafe {
        AddVectoredExceptionHandler(1, Some(on_write));
    });
}

/// Whether `at` is committed read-write memory now.
fn writable(at: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_READWRITE, VirtualQuery,
    };
    // SAFETY: plain data filled in by the query.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a valid out pointer of the right size.
    let n = unsafe { VirtualQuery(at as *const _, &mut info, size_of_val(&info)) };
    n != 0 && info.State == MEM_COMMIT && info.Protect == PAGE_READWRITE
}

/// For tests outside this file that read title memory in place or depend
/// on its mapping staying put: held, no guard test changes the mapping (they
/// hold the same lock while they do).
#[cfg(test)]
pub(crate) fn serial_with_guard_tests() -> std::sync::MutexGuard<'static, ()> {
    tests::lock_guards()
}

/// For tests: whether the view guards and the journal know nothing of
/// `[start, start + bytes)`. Other tests free memory they marked or noted
/// written, and an address reused would answer from what they left.
#[cfg(test)]
pub(crate) fn untouched(start: usize, bytes: usize) -> bool {
    let (first, last) = (start / PAGE, (start + bytes).div_ceil(PAGE));
    !(first..last).any(|b| known_view(b * PAGE))
        && JOURNAL
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .written
            .range(first..last)
            .next()
            .is_none()
}

#[cfg(test)]
mod parts_tests {
    use super::*;

    #[test]
    fn the_parts_of_a_page_written_since_it_was_taken() {
        let mut j = Journal {
            written: BTreeMap::new(),
            parts: std::collections::HashMap::with_hasher(std::hash::BuildHasherDefault::new()),
            found: Vec::new(),
            ahead: Vec::new(),
            ahead_views: Vec::new(),
            ahead_asked: 0,
            kept: false,
        };
        assert_eq!(j.parts_since(7, 0), WHOLE);
        assert_eq!(j.note(7, WHOLE, 3), 0);
        // Parts 0 and 5 at 4, part 5 again at 6: at 2 the whole page is due.
        assert_eq!(j.note(7, 0b10_0001, 4), 3);
        assert_eq!(j.note(7, 0b10_0000, 6), 4);
        assert_eq!(j.parts_since(7, 2), WHOLE);
        assert_eq!(j.parts_since(7, 3), 0b10_0001);
        assert_eq!(j.parts_since(7, 4), 0b10_0000);
        assert_eq!(j.parts_since(7, 5), 0b10_0000);
        assert_eq!(j.parts_since(7, 6), 0);
        // Written whole: all of it, whoever took it before.
        assert_eq!(j.note(7, WHOLE, 8), 6);
        assert_eq!(j.parts_since(7, 7), WHOLE);
        // A page never written before: what was not named was never written.
        assert_eq!(j.note(9, 0b100, 9), 0);
        assert_eq!(j.parts_since(9, 5), 0b100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
    };
    const MEM_WRITE_WATCH: u32 = 0x20_0000;

    /// Guard/VEH tests mutate process-wide `VirtualProtect` and global
    /// `GUARDS`/`VIEWS`; serialize them so `cargo test --workspace` stays
    /// deterministic when tests run in parallel.
    static GUARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(super) fn lock_guards() -> std::sync::MutexGuard<'static, ()> {
        GUARD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A fresh write-watched allocation of `bytes` at an address the view
    /// guards and the journal know nothing of: other tests free memory they
    /// marked or noted written, and an address reused would answer from
    /// what they left. Addresses that are known are kept (never freed), so
    /// they are not handed out again. 0 if none was found.
    pub(super) fn fresh_watched(bytes: usize) -> usize {
        static KEPT: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
        for _ in 0..4096 {
            // SAFETY: a private allocation, freed by the caller or kept here.
            let p = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    bytes,
                    MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH,
                    PAGE_READWRITE,
                )
            } as usize;
            if p == 0 {
                return 0;
            }
            if untouched(p, bytes) {
                return p;
            }
            KEPT.lock().unwrap_or_else(|e| e.into_inner()).push(p);
        }
        0
    }

    /// The guards asked as [`writes_into`] asks them: with what
    /// [`known_view`] says of the range's first block.
    fn guard(start: usize, end: usize) -> Option<Vec<usize>> {
        super::guard(start, end, known_view(start))
    }

    /// Threads that write pages and ask at the same time, about ranges that
    /// overlap: each sees its own write in every answer that follows it,
    /// whichever thread's asking cleared the kernel's bit (with
    /// `VICTORY_WATCH_APART=1` the askings go side by side).
    #[test]
    fn askings_side_by_side_lose_no_write() {
        const PAGES: usize = 16;
        // A second asking between an answer and its entry would find the
        // kernel's bits cleared and the journal not told yet.
        let _ = WIDEN.set(300);
        // SAFETY: a fresh write-watched allocation, freed at the end.
        let p = unsafe {
            VirtualAlloc(std::ptr::null(), PAGES * PAGE, MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH, PAGE_READWRITE)
        } as usize;
        assert_ne!(p, 0);
        std::thread::scope(|s| {
            for t in 0..8usize {
                s.spawn(move || {
                    let mut written = Vec::new();
                    for i in 0..300usize {
                        let page = (t * 2 + i) % PAGES;
                        // SAFETY: committed pages of ours.
                        unsafe { ((p + page * PAGE + t * 8) as *mut u64).write_volatile(i as u64) };
                        // A range that holds the page, of one to six pages.
                        let len = 1 + (i + t) % 6;
                        let from = page.saturating_sub(i % len).min(PAGES - len);
                        let now = writes_into(p + from * PAGE, p + (from + len) * PAGE, false, &mut written)
                            .expect("watched");
                        let mine = p / PAGE + page;
                        assert!(
                            written.iter().any(|&(q, w)| q == mine && w <= now),
                            "thread {t}, step {i}: page {page} written and not reported"
                        );
                    }
                });
            }
        });
        // Kept to the end of the process: the journal remembers these pages
        // as written, and another test given the same addresses would find
        // them there.
    }

    #[test]
    fn written_pages_are_reported_to_every_asker() {
        // A fresh write-watched allocation, freed at the end.
        let p = fresh_watched(4 * PAGE);
        assert_ne!(p, 0);
        let (start, end) = (p, p + 4 * PAGE);
        let (_, before) = writes(start, end, false).expect("watched");
        assert!(before.is_empty());
        // SAFETY: committed pages of ours.
        unsafe { ((p + PAGE + 8) as *mut u8).write(1) };
        let (now, after) = writes(start, end, false).expect("watched");
        assert_eq!(after, [(p / PAGE + 1, now)]);
        // Another asker after the kernel's bit was cleared still sees it.
        let (later, again) = writes(p + PAGE, p + 2 * PAGE, false).expect("watched");
        assert!(later > now);
        assert_eq!(again, [(p / PAGE + 1, now)]);
        let mut plain = vec![0u8; PAGE];
        assert!(
            writes(
                plain.as_mut_ptr() as usize,
                plain.as_ptr() as usize + PAGE,
                true
            )
            .is_none()
        );
        // SAFETY: our allocation.
        unsafe { VirtualFree(p as *mut _, 0, MEM_RELEASE) };
    }

    /// With the guards asked once for a submission, askings about the view
    /// are answered from the journal until that ends: a write after it is
    /// reported by the next one. It says nothing to an asking that is not
    /// about guarded memory.
    #[test]
    fn the_guards_answer_once_for_a_submission() {
        use windows_sys::Win32::System::Memory::{CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile};
        // SAFETY: a pagefile section of two blocks, mapped read-write.
        let view = unsafe {
            let section = CreateFileMappingW(-1isize as _, std::ptr::null(), PAGE_READWRITE, 0, 2 * BLOCK as u32, std::ptr::null());
            assert!(!section.is_null());
            MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, 2 * BLOCK)
        };
        let p = view.Value as usize;
        assert_ne!(p, 0);
        // SAFETY: committed pages of the view; a write to a guarded one is
        // answered by the handler.
        let put = |at: usize| unsafe { (at as *mut u8).write_volatile(1) };
        let end = p + 2 * BLOCK;
        let first = writes(p, end, true).expect("a view");
        assert!(first.1.iter().all(|w| w.1 == first.0));
        put(p + BLOCK);
        ask_now(&[(p, end)], true);
        let (now, written) = writes(p + BLOCK, end, true).expect("a view");
        assert_eq!(written, [((p + BLOCK) / PAGE, now)]);
        assert_eq!(asked(p, end).map(|a| a.0), Some(now));
        put(p);
        let (same, written) = writes(p, end, true).expect("a view");
        assert_eq!((same, written[0]), (now, (p / PAGE, first.0)), "after the asking: for the next one");
        assert!(writes(p, end, false).is_none(), "not an asking about guarded memory");
        ask_once_end();
        let (later, written) = writes(p, end, true).expect("a view");
        assert!(later > now);
        assert_eq!(written, [(p / PAGE, later), ((p + BLOCK) / PAGE, now)]);
        // (The view stays: what the journal noted as written there would be
        // told to the next test that gets the address.)
        title_memory_remapped(p, 2 * BLOCK);
    }

    /// A kernel read into a guarded view block fails; opened first, it
    /// lands and the block counts as written.
    #[test]
    fn kernel_reads_into_guarded_blocks_after_opening() {
        let _serial = lock_guards();
        use std::io::{Read, Seek, Write};
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile, UnmapViewOfFile,
        };
        // SAFETY: a pagefile section of two blocks, mapped read-write.
        let view = unsafe {
            let section = CreateFileMappingW(
                -1isize as _,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                (2 * BLOCK) as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, 2 * BLOCK).Value as usize
        };
        assert_ne!(view, 0);
        assert!(guard(view, view + 2 * BLOCK).is_some()); // now read-only
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"from the disk").unwrap();
        // `ReadFile` inside: the kernel writes the view.
        let read = |file: &mut std::fs::File| {
            file.rewind().unwrap();
            // SAFETY: 13 bytes of our view, nothing else refers to them.
            let into = unsafe { std::slice::from_raw_parts_mut((view + 100) as *mut u8, 13) };
            file.read(into).is_ok_and(|n| n == 13)
        };
        assert!(
            !read(&mut file),
            "the kernel does not fault into our handler"
        );
        writing(view + 100, 13);
        assert!(read(&mut file));
        // SAFETY: the bytes just read.
        let landed = unsafe { std::slice::from_raw_parts((view + 100) as *const u8, 13) };
        assert_eq!(landed, b"from the disk");
        assert_eq!(
            guard(view, view + 2 * BLOCK),
            Some((view / PAGE..view / PAGE + 1).collect())
        );
        // SAFETY: our view.
        unsafe {
            UnmapViewOfFile(
                windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: view as *mut _,
                },
            )
        };
        title_memory_remapped(view, 2 * BLOCK);
    }

    #[test]
    fn writes_in_partly_committed_view_blocks_are_reported() {
        let _serial = lock_guards();
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
            SEC_RESERVE, UnmapViewOfFile,
        };

        const SIZE: usize = 3 * BLOCK;
        struct View(MEMORY_MAPPED_VIEW_ADDRESS);
        impl Drop for View {
            fn drop(&mut self) {
                title_memory_remapped(self.0.Value as usize, SIZE);
                // SAFETY: our view is still mapped.
                unsafe { UnmapViewOfFile(self.0) };
            }
        }
        // SAFETY: a new reserved pagefile section. The view holds its own
        // reference after we close the handle, including on assertion failure.
        let view = unsafe {
            let section = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE | SEC_RESERVE,
                0,
                SIZE as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            let view = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, SIZE);
            CloseHandle(section);
            assert!(!view.Value.is_null());
            View(view)
        };
        let start = view.0.Value as usize;
        let commit = |at: usize, len| {
            // SAFETY: these pages belong to our reserved view.
            let committed = unsafe { VirtualAlloc(at as _, len, MEM_COMMIT, PAGE_READWRITE) };
            assert_eq!(committed as usize, at);
        };
        let early = start + BLOCK + 3 * KERNEL_PAGE;
        let late = start + 2 * BLOCK + 3 * KERNEL_PAGE;
        commit(start, BLOCK);
        commit(early, KERNEL_PAGE);
        let refresh = || guard(start, start + SIZE).unwrap();
        assert_eq!(refresh(), [start / PAGE, early / PAGE, late / PAGE]);
        // The last block is still entirely reserved.
        assert!(!refresh().contains(&(late / PAGE)));

        commit(late, KERNEL_PAGE); // Ordinary commit, without a runtime mapping notification.
        for at in [early, late] {
            assert!(writable(at));
            // SAFETY: each address is inside its committed read-write page.
            unsafe { (at as *mut u8).write_volatile(1) };
        }
        // With the view set: found at the next look at the blocks parked as
        // uncommitted that are no placeholder.
        SUBMISSIONS.fetch_add(park_epoch(), std::sync::atomic::Ordering::Relaxed);
        assert_eq!(refresh(), [early / PAGE, late / PAGE]);
    }

    /// Blocks found uncommitted are parked: not asked about again until the
    /// title commits them through the runtime (found at once) or the next
    /// look at the parked blocks that are no placeholder (a commit no one
    /// tells of).
    #[test]
    fn parked_blocks_come_back_when_committed() {
        let _serial = lock_guards();
        if !view_set() {
            return;
        }
        // SAFETY: a fresh reservation of two blocks, nothing committed.
        let p = unsafe { VirtualAlloc(std::ptr::null(), 2 * BLOCK, MEM_RESERVE, PAGE_READWRITE) } as usize;
        assert_ne!(p, 0);
        let pages = |block: usize| (p + block * BLOCK) / PAGE..(p + (block + 1) * BLOCK) / PAGE;
        let all: Vec<usize> = (0..2).flat_map(pages).collect();
        // Nothing there: a view, its blocks reported once, then parked.
        assert_eq!(guard(p, p + 2 * BLOCK), Some(all));
        assert_eq!(guard(p, p + 2 * BLOCK), Some(Vec::new()));
        {
            // No placeholder: looked at again each epoch.
            let g = guards();
            assert!(g.parked.contains(&(p / BLOCK)) && g.parked_ordinary.contains(&(p / BLOCK)));
        }
        // SAFETY: the first block of our reservation.
        assert_eq!(unsafe { VirtualAlloc(p as _, BLOCK, MEM_COMMIT, PAGE_READWRITE) } as usize, p);
        title_memory_committed(p, BLOCK);
        assert_eq!(guard(p, p + 2 * BLOCK), Some(pages(0).collect()));
        assert_eq!(guard(p, p + 2 * BLOCK), Some(Vec::new()));
        // SAFETY: the second block of our reservation.
        assert_eq!(unsafe { VirtualAlloc((p + BLOCK) as _, BLOCK, MEM_COMMIT, PAGE_READWRITE) } as usize, p + BLOCK);
        SUBMISSIONS.fetch_add(park_epoch(), std::sync::atomic::Ordering::Relaxed);
        assert_eq!(guard(p, p + 2 * BLOCK), Some(pages(1).collect()));
        assert_eq!(guard(p, p + 2 * BLOCK), Some(Vec::new()));
        title_memory_remapped(p, 2 * BLOCK);
        // SAFETY: our reservation, no longer known to the guards.
        unsafe { VirtualFree(p as _, 0, MEM_RELEASE) };
    }

    /// A range of view memory whose first block is not mapped is a view
    /// all the same: its blocks are reported once, then the ones written
    /// or mapped since.
    #[test]
    fn a_range_that_begins_in_an_unmapped_block_is_guarded() {
        let _serial = lock_guards();
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, MEM_PRESERVE_PLACEHOLDER, MEM_REPLACE_PLACEHOLDER,
            MEM_RESERVE_PLACEHOLDER, MapViewOfFile3, PAGE_NOACCESS, UnmapViewOfFile2,
            VirtualAlloc2,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: a fresh placeholder of three blocks and a pagefile
        // section of three; the middle block is mapped first.
        let (p, section) = unsafe {
            let p = VirtualAlloc2(
                GetCurrentProcess(),
                std::ptr::null(),
                3 * BLOCK,
                MEM_RESERVE | MEM_RESERVE_PLACEHOLDER,
                PAGE_NOACCESS,
                std::ptr::null_mut(),
                0,
            ) as usize;
            assert_ne!(p, 0);
            let section = CreateFileMappingW(
                -1isize as _,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                (3 * BLOCK) as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            (p, section)
        };
        let map = |block: usize| {
            // SAFETY: splits the placeholder at the block and maps the
            // section's block there.
            let view = unsafe {
                VirtualFree(
                    (p + block * BLOCK) as *mut _,
                    BLOCK,
                    MEM_RELEASE | MEM_PRESERVE_PLACEHOLDER,
                );
                MapViewOfFile3(
                    section,
                    GetCurrentProcess(),
                    (p + block * BLOCK) as *const _,
                    (block * BLOCK) as u64,
                    BLOCK,
                    MEM_REPLACE_PLACEHOLDER,
                    PAGE_READWRITE,
                    std::ptr::null_mut(),
                    0,
                )
            };
            assert_eq!(view.Value as usize, p + block * BLOCK);
            view
        };
        let pages = |block: usize| (p + block * BLOCK) / PAGE..(p + (block + 1) * BLOCK) / PAGE;
        let all: Vec<usize> = (0..3).flat_map(pages).collect();
        // Nothing mapped at all: a view with every block absent.
        assert!(is_view(p, p + 3 * BLOCK));
        let middle = map(1);
        title_memory_remapped(p + BLOCK, BLOCK);
        // The range begins in the placeholder, its middle block is a view.
        assert!(is_view(p, p + 3 * BLOCK));
        assert_eq!(guard(p, p + 3 * BLOCK), Some(all));
        assert_eq!(guard(p, p + 3 * BLOCK), Some(Vec::new()), "nothing written since");
        // SAFETY: inside the mapped middle block.
        unsafe { ((p + BLOCK + 8) as *mut u8).write_volatile(1) };
        assert_eq!(guard(p, p + 3 * BLOCK), Some(pages(1).collect()));
        let first = map(0);
        title_memory_remapped(p, BLOCK);
        assert_eq!(guard(p, p + 3 * BLOCK), Some(pages(0).collect()));
        assert_eq!(guard(p, p + 3 * BLOCK), Some(Vec::new()));
        // Memory of ours that is no view stays none.
        let mut local = [0u8; 16];
        let at = local.as_mut_ptr() as usize;
        assert!(!is_view(at, at + local.len()));
        // SAFETY: our views and placeholder.
        unsafe {
            UnmapViewOfFile2(GetCurrentProcess(), first, MEM_PRESERVE_PLACEHOLDER);
            UnmapViewOfFile2(GetCurrentProcess(), middle, MEM_PRESERVE_PLACEHOLDER);
            for block in 0..3 {
                VirtualFree((p + block * BLOCK) as *mut _, 0, MEM_RELEASE);
            }
            windows_sys::Win32::Foundation::CloseHandle(section);
        }
        title_memory_remapped(p, 3 * BLOCK);
    }

    /// Blocks of a placeholder between views are reported once, then not
    /// again (they cannot be guarded) until the runtime maps them.
    #[test]
    fn unmapped_blocks_are_reported_once_until_mapped() {
        let _serial = lock_guards();
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, MEM_PRESERVE_PLACEHOLDER, MEM_REPLACE_PLACEHOLDER,
            MEM_RESERVE_PLACEHOLDER, MapViewOfFile3, PAGE_NOACCESS, UnmapViewOfFile2,
            VirtualAlloc2,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: a fresh placeholder of three blocks and a pagefile
        // section of two; block 0 shows the section's first block, as the
        // runtime maps physical pages.
        let (p, section) = unsafe {
            let p = VirtualAlloc2(
                GetCurrentProcess(),
                std::ptr::null(),
                3 * BLOCK,
                MEM_RESERVE | MEM_RESERVE_PLACEHOLDER,
                PAGE_NOACCESS,
                std::ptr::null_mut(),
                0,
            ) as usize;
            assert_ne!(p, 0);
            let section = CreateFileMappingW(
                -1isize as _,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                (2 * BLOCK) as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            (p, section)
        };
        let map = |block: usize| {
            // SAFETY: splits the placeholder at the block and maps the
            // section's block there.
            let view = unsafe {
                VirtualFree(
                    (p + block * BLOCK) as *mut _,
                    BLOCK,
                    MEM_RELEASE | MEM_PRESERVE_PLACEHOLDER,
                );
                MapViewOfFile3(
                    section,
                    GetCurrentProcess(),
                    (p + block * BLOCK) as *const _,
                    (block * BLOCK) as u64,
                    BLOCK,
                    MEM_REPLACE_PLACEHOLDER,
                    PAGE_READWRITE,
                    std::ptr::null_mut(),
                    0,
                )
            };
            assert_eq!(view.Value as usize, p + block * BLOCK);
            view
        };
        let first = map(0);
        let pages = |block: usize| (p + block * BLOCK) / PAGE..(p + (block + 1) * BLOCK) / PAGE;
        let all: Vec<usize> = (0..3).flat_map(pages).collect();
        assert_eq!(guard(p, p + 3 * BLOCK), Some(all));
        assert_eq!(
            guard(p, p + 3 * BLOCK),
            Some(Vec::new()),
            "nothing written since"
        );
        if view_set() {
            // Placeholders: parked until the runtime maps them, never asked
            // about again meanwhile.
            let g = guards();
            for block in [1, 2].map(|k| p / BLOCK + k) {
                assert!(g.parked.contains(&block) && !g.parked_ordinary.contains(&block));
            }
        }
        let second = map(1);
        title_memory_remapped(p + BLOCK, BLOCK);
        assert_eq!(guard(p, p + 3 * BLOCK), Some(pages(1).collect()));
        assert_eq!(guard(p, p + 3 * BLOCK), Some(Vec::new()));
        // SAFETY: our views and placeholder.
        unsafe {
            UnmapViewOfFile2(GetCurrentProcess(), first, MEM_PRESERVE_PLACEHOLDER);
            UnmapViewOfFile2(GetCurrentProcess(), second, MEM_PRESERVE_PLACEHOLDER);
            for block in 0..3 {
                VirtualFree((p + block * BLOCK) as *mut _, 0, MEM_RELEASE);
            }
            windows_sys::Win32::Foundation::CloseHandle(section);
        }
        title_memory_remapped(p, 3 * BLOCK);
    }

    /// A pagefile section mapped twice: two views of the same physical
    /// pages, the second left for the test; the section handle, the two
    /// addresses.
    fn two_views(bytes: usize) -> (usize, usize, usize) {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MapViewOfFile,
        };
        // SAFETY: a fresh pagefile section, mapped twice; the views keep
        // the section alive and the handle is closed by the caller.
        unsafe {
            let section = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                bytes as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            let one = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, bytes).Value as usize;
            let two = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, bytes).Value as usize;
            assert_ne!(one, 0);
            assert_ne!(two, 0);
            crate::inplace::title_memory_view(one, bytes, section as usize, 0);
            crate::inplace::title_memory_view(two, bytes, section as usize, 0);
            (section as usize, one, two)
        }
    }

    /// The section of [`two_views`] is closed and its views unmapped; the
    /// guard is told they are gone first.
    fn drop_views(section: usize, views: &[usize], bytes: usize) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        for &view in views {
            title_memory_remapped(view, bytes);
            // SAFETY: a view of the section mapped by `two_views`.
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: view as *mut _,
                })
            };
        }
        // SAFETY: the section's last reference.
        unsafe { CloseHandle(section as *mut std::ffi::c_void) };
    }

    /// The title wrote the pages of a guarded texture through another view
    /// of them: the fault at that view is the guard's to see, and the
    /// guarded view's next check reports the pages written.
    #[test]
    fn a_write_through_another_view_is_reported_at_the_guarded_one() {
        let _serial = lock_guards();
        const SIZE: usize = 2 * BLOCK;
        let (section, guarded, other) = two_views(SIZE);
        let pages = |block: usize| (guarded + block * BLOCK) / PAGE;
        // The first check takes it in (and every view of its pages).
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(0), pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()), "nothing written yet");
        // Writing through the other view faults into the handler, which
        // opens that view and the guarded one: the next check sees it.
        // SAFETY: the second view, read-write once the handler has opened it.
        unsafe { ((other + BLOCK + 8) as *mut u8).write(1) };
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()));
        drop_views(section, &[guarded, other], SIZE);
    }

    /// A view of guarded pages is mapped anew: it is writable, so the
    /// guarded view checks its pages again (and guards the new view).
    #[test]
    fn a_view_mapped_beside_a_guarded_one_makes_it_check_again() {
        let _serial = lock_guards();
        const SIZE: usize = 2 * BLOCK;
        let (section, guarded, other) = two_views(SIZE);
        let pages = |block: usize| (guarded + block * BLOCK) / PAGE;
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(0), pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()), "nothing written yet");
        title_memory_remapped(other, SIZE);
        crate::inplace::title_memory_view(other, SIZE, section, 0);
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(0), pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()));
        drop_views(section, &[guarded, other], SIZE);
    }

    /// A turn's share of the clean blocks is the words of its number, with
    /// the holes left out: making a hole due would question Windows.
    #[test]
    fn a_turn_takes_its_words_of_clean_blocks_without_the_holes() {
        let (mut clean, mut absent) = (Blocks::new(), Blocks::new());
        // Words 0, 1 and 3 hold clean blocks; block 65 (word 1) is a hole.
        for block in [0, 5, 64, 65, 70, 192] {
            clean.insert(block);
        }
        absent.insert(65);
        assert_eq!(due_bits(&clean, &absent, 2, 0), vec![(0, 1 | 1 << 5)]);
        assert_eq!(due_bits(&clean, &absent, 2, 1), vec![(1, 1 | 1 << 6), (3, 1)]);
        // A word of holes alone has no share.
        absent.insert(64);
        absent.insert(70);
        assert_eq!(due_bits(&clean, &absent, 2, 1), vec![(3, 1)]);
        // Every turn of a round together takes each clean block once.
        let taken: u32 = (0..7).flat_map(|turn| due_bits(&clean, &Blocks::new(), 7, turn)).map(|(_, bits)| bits.count_ones()).sum();
        assert_eq!(taken, 6);
    }

    /// A write of the runtime's own, told at one view of the pages
    /// (`writing`): the other views' next checks report the pages written.
    #[test]
    fn a_write_told_at_one_view_is_reported_at_the_others() {
        let _serial = lock_guards();
        const SIZE: usize = 2 * BLOCK;
        let (section, guarded, other) = two_views(SIZE);
        let pages = |block: usize| (guarded + block * BLOCK) / PAGE;
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(0), pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()), "nothing written yet");
        writing(other + BLOCK, BLOCK);
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(1)]));
        assert_eq!(guard(guarded, guarded + SIZE), Some(Vec::new()));
        // Half a page and the next: both blocks, not just the first's.
        writing(other + BLOCK / 2, BLOCK);
        assert_eq!(guard(guarded, guarded + SIZE), Some(vec![pages(0), pages(1)]));
        drop_views(section, &[guarded, other], SIZE);
    }

    /// Both views of the pages are checked, each for a copy of its own
    /// (the title shows one physical page at several addresses, and each
    /// address has its host copy): a write through either is reported at
    /// both, whichever is checked first. Guarding the pages for the first
    /// view's check must not note the second view clean, or its copy would
    /// keep the bytes from before the write.
    #[test]
    fn a_write_is_reported_at_every_checked_view() {
        let _serial = lock_guards();
        const SIZE: usize = 2 * BLOCK;
        let (section, one, two) = two_views(SIZE);
        let pages = |view: usize, block: usize| (view + block * BLOCK) / PAGE;
        assert_eq!(guard(one, one + SIZE), Some(vec![pages(one, 0), pages(one, 1)]));
        assert_eq!(guard(two, two + SIZE), Some(vec![pages(two, 0), pages(two, 1)]), "the second view's first check");
        assert_eq!(guard(one, one + SIZE), Some(Vec::new()), "nothing written yet");
        assert_eq!(guard(two, two + SIZE), Some(Vec::new()), "nothing written yet");
        // SAFETY: the second view, read-write once the handler has opened it.
        unsafe { ((two + BLOCK + 8) as *mut u8).write(1) };
        assert_eq!(guard(one, one + SIZE), Some(vec![pages(one, 1)]));
        assert_eq!(guard(two, two + SIZE), Some(vec![pages(two, 1)]), "written through this view, checked second");
        // SAFETY: the first view, as above.
        unsafe { ((one + 8) as *mut u8).write(1) };
        assert_eq!(guard(two, two + SIZE), Some(vec![pages(two, 0)]));
        assert_eq!(guard(one, one + SIZE), Some(vec![pages(one, 0)]), "written through this view, checked second");
        assert_eq!(guard(one, one + SIZE), Some(Vec::new()));
        assert_eq!(guard(two, two + SIZE), Some(Vec::new()));
        drop_views(section, &[one, two], SIZE);
    }

    /// A write through a view that no copy checks: the checked view is
    /// noted written and left read-only, its check reports the pages and
    /// guards the written view again, so a second write through it is seen
    /// as well.
    #[test]
    fn a_view_no_copy_checks_is_guarded_again_by_the_checked_one() {
        let _serial = lock_guards();
        let (section, checked, other) = two_views(BLOCK);
        assert_eq!(guard(checked, checked + BLOCK), Some(vec![checked / PAGE]));
        assert!(!writable(other), "guarded with the checked view");
        // SAFETY: the other view, read-write once the handler has opened it.
        unsafe { (other as *mut u8).write(1) };
        assert!(writable(other), "opened by the handler");
        assert!(!writable(checked), "noted written, left read-only");
        assert_eq!(guard(checked, checked + BLOCK), Some(vec![checked / PAGE]));
        assert!(!writable(other), "guarded again by the check");
        // SAFETY: as above.
        unsafe { (other as *mut u8).write(2) };
        assert_eq!(guard(checked, checked + BLOCK), Some(vec![checked / PAGE]));
        assert_eq!(guard(checked, checked + BLOCK), Some(Vec::new()));
        // SAFETY: the checked view, opened by the handler.
        unsafe { (checked as *mut u8).write(3) };
        assert_eq!(guard(checked, checked + BLOCK), Some(vec![checked / PAGE]));
        drop_views(section, &[checked, other], BLOCK);
    }

    /// A page shown in more than `MANY_VIEWS` views is guarded at each by
    /// itself: the others stay writable, and nothing looks through them all.
    #[test]
    fn a_page_of_many_views_is_guarded_one_view_at_a_time() {
        let _serial = lock_guards();
        let (section, one, two) = two_views(BLOCK);
        let filler = one + (1 << 45);
        for n in 0..MANY_VIEWS {
            crate::inplace::title_memory_view(filler + n * BLOCK, BLOCK, section, 0);
        }
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));
        assert!(writable(two), "another view of it is left alone");
        // SAFETY: the guarded view, opened by the handler.
        unsafe { ((one + 8) as *mut u8).write(1) };
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));
        title_memory_remapped(filler, MANY_VIEWS * BLOCK);
        drop_views(section, &[one, two], BLOCK);
    }

    #[test]
    fn crossing_many_views_upward_releases_aliases_and_reports_dirty() {
        let _serial = lock_guards();
        let (section, one, two) = two_views(BLOCK);
        // Bring view count to exact MANY_VIEWS (64): one + two + 62 fillers.
        let filler = one + (1 << 45);
        for n in 0..MANY_VIEWS - 2 {
            crate::inplace::title_memory_view(filler + n * BLOCK, BLOCK, section, 0);
        }
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));
        assert_eq!(guard(one, one + BLOCK), Some(Vec::new()));
        assert!(!writable(two), "two is guarded alongside one at MANY_VIEWS");
        assert!(guards().aliased.contains(&(one / BLOCK)));

        // Adding the (MANY_VIEWS + 1)-th view crosses the threshold: existing
        // guarded aliases must be opened and removed from g.aliased so `two`
        // is not stuck read-only and `one` does not stay spuriously clean.
        let extra = filler + (MANY_VIEWS - 2) * BLOCK;
        crate::inplace::title_memory_view(extra, BLOCK, section, 0);
        assert!(writable(two), "sibling alias must be released when crossing MANY_VIEWS");
        assert!(!guards().aliased.contains(&(one / BLOCK)));
        assert!(!guards().aliased.contains(&(two / BLOCK)));
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));

        title_memory_remapped(filler, (MANY_VIEWS - 1) * BLOCK);
        drop_views(section, &[one, two], BLOCK);
    }

    #[test]
    fn crossing_many_views_downward_reopens_remaining_views_for_shared_guarding() {
        let _serial = lock_guards();
        let (section, one, two) = two_views(BLOCK);
        // Start above MANY_VIEWS (65 views): one + two + 63 fillers.
        let filler = one + (1 << 45);
        for n in 0..MANY_VIEWS - 1 {
            crate::inplace::title_memory_view(filler + n * BLOCK, BLOCK, section, 0);
        }
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));
        assert_eq!(guard(one, one + BLOCK), Some(Vec::new()));
        assert!(writable(two), "above MANY_VIEWS, two is left writable");

        // Unmap the fillers, dropping the page back to 2 views (<= MANY_VIEWS).
        title_memory_remapped(filler, (MANY_VIEWS - 1) * BLOCK);
        // `one` must be reopened so the next guard protects `two` alongside it.
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));
        assert!(!writable(two), "after dropping below MANY_VIEWS, two is guarded with one");
        assert_eq!(guard(one, one + BLOCK), Some(Vec::new()));
        // Writing through `two` now faults and notifies `one`.
        // SAFETY: `two` is a live mapped view of `section`; the VEH reopens it.
        unsafe { ((two + 8) as *mut u8).write(1) };
        assert_eq!(guard(one, one + BLOCK), Some(vec![one / PAGE]));

        drop_views(section, &[one, two], BLOCK);
    }

    /// A write or mapping that runs past the end of the address space (a
    /// title's `fread` with a huge count) covers up to the end; the sum used
    /// to overflow. Up there no test has guarded blocks.
    #[test]
    fn ranges_past_the_address_space_end_do_not_overflow() {
        let _serial = lock_guards();
        let top = usize::MAX - 16;
        writing(top, PAGE);
        title_memory_remapped(top, usize::MAX);
    }

    /// An uncommitted address noted in `g.absent` that is later allocated as
    /// `MEM_WRITE_WATCH` or ordinary `MEM_PRIVATE` memory must not be hijacked
    /// into `PAGE_READONLY` guarded memory.
    #[test]
    fn absent_block_reallocated_as_private_memory_is_not_guarded() {
        let _serial = lock_guards();
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
            VirtualFree,
        };
        const MEM_WRITE_WATCH: u32 = 0x20_0000;

        // 1. Reserve an uncommitted 64 KiB block and observe it via `writes`,
        //    which records the block in `g.absent`.
        let reserved = unsafe { VirtualAlloc(std::ptr::null(), PAGE, MEM_RESERVE, PAGE_NOACCESS) };
        assert!(!reserved.is_null());
        let addr = reserved as usize;
        assert!(writes(addr, addr + PAGE, true).is_some());
        assert!(guards().absent.contains(addr / BLOCK));
        assert_ne!(unsafe { VirtualFree(reserved, 0, MEM_RELEASE) }, 0);

        // 2. Reallocate the exact same virtual address as MEM_WRITE_WATCH private memory.
        let watched = unsafe {
            VirtualAlloc(
                reserved,
                PAGE,
                MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH,
                PAGE_READWRITE,
            )
        };
        assert_eq!(watched, reserved);
        assert!(writes(addr, addr + PAGE, true).is_some());
        assert!(
            writable(addr),
            "MEM_WRITE_WATCH private memory at a formerly absent address must remain writable"
        );
        assert!(!guards().protected.contains(&(addr / BLOCK)));
        assert_ne!(unsafe { VirtualFree(watched, 0, MEM_RELEASE) }, 0);

        // 3. Re-mark `addr` absent while free, then commit ordinary MEM_PRIVATE memory there.
        let reserved2 = unsafe { VirtualAlloc(reserved, PAGE, MEM_RESERVE, PAGE_NOACCESS) };
        assert_eq!(reserved2, reserved);
        assert!(writes(addr, addr + PAGE, true).is_some());
        assert!(guards().absent.contains(addr / BLOCK));
        assert_ne!(unsafe { VirtualFree(reserved2, 0, MEM_RELEASE) }, 0);

        let plain = unsafe { VirtualAlloc(reserved, PAGE, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) };
        assert_eq!(plain, reserved);
        assert!(
            writes(addr, addr + PAGE, true).is_none(),
            "ordinary MEM_PRIVATE memory at a formerly absent address must not be treated as a view"
        );
        assert!(writable(addr));
        assert!(!guards().protected.contains(&(addr / BLOCK)));
        // (Left allocated: what was noted as written there would be told to
        // the next test that gets the address.)
        // Clean up the test's absent entry.
        guards().absent.remove(addr / BLOCK);
    }

    /// The same behind a view: a block of a range that starts with a view
    /// turns into private memory. It stays writable however often the range
    /// is asked about, and counts as written each time (nothing watches it).
    #[test]
    fn private_memory_behind_a_view_is_never_guarded() {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
            MapViewOfFileEx, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        };
        // SAFETY: memory of this test alone: a view of one block and a
        // reservation right behind it, where a larger one was just released.
        unsafe {
            let section = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                BLOCK as u32,
                std::ptr::null(),
            );
            assert!(!section.is_null());
            // Another test's thread may take the released addresses first.
            let mut found = None;
            for _ in 0..64 {
                let both = VirtualAlloc(std::ptr::null(), 2 * BLOCK, MEM_RESERVE, PAGE_NOACCESS);
                assert!(!both.is_null());
                assert_ne!(VirtualFree(both, 0, MEM_RELEASE), 0);
                let view = MapViewOfFileEx(section, FILE_MAP_WRITE, 0, 0, BLOCK, both).Value;
                let behind = both as usize + BLOCK;
                let reserved = VirtualAlloc(behind as *const _, BLOCK, MEM_RESERVE, PAGE_NOACCESS);
                if view == both && reserved as usize == behind {
                    found = Some((both as usize, behind, reserved));
                    break;
                }
            }
            let (start, behind, reserved) = found.expect("a view with a reservation behind it");
            assert!(writes(start, start + 2 * BLOCK, true).is_some());
            assert!(guards().absent.contains(behind / BLOCK));
            let plain = VirtualAlloc(reserved, BLOCK, MEM_COMMIT, PAGE_READWRITE);
            assert_eq!(plain, reserved);
            for round in 0..3 {
                let (_, written) = writes(start, start + 2 * BLOCK, true).unwrap();
                assert!(writable(behind), "round {round}: private memory was made read-only");
                assert!(!guards().protected.contains(&(behind / BLOCK)), "round {round}");
                assert!(
                    written.iter().any(|&(page, _)| page * PAGE == behind),
                    "round {round}: what nothing watches counts as written"
                );
            }
            // The two blocks stay taken: what was noted as written there
            // would be told to the next test that gets these addresses.
            guards().absent.remove(behind / BLOCK);
            CloseHandle(section);
        }
    }
}

#[cfg(test)]
mod range_tests {
    use super::{add_range, covered};

    #[test]
    fn ranges_stay_in_order_and_joined() {
        let mut ranges = Vec::new();
        for (a, b) in [(10, 20), (30, 40), (0, 5), (40, 45), (19, 31), (7, 7), (50, 60), (5, 6)] {
            add_range(&mut ranges, a, b);
            assert!(ranges.windows(2).all(|w| w[0].1 < w[1].0), "{ranges:?}");
        }
        assert_eq!(ranges, [(0, 6), (10, 45), (50, 60)]);
        assert!(covered(&ranges, 10, 45) && covered(&ranges, 0, 1) && covered(&ranges, 59, 60));
        assert!(!covered(&ranges, 5, 7) && !covered(&ranges, 9, 11) && !covered(&ranges, 44, 46));
        assert!(!covered(&ranges, 45, 51) && !covered(&ranges, 60, 61) && !covered(&[], 1, 2));
    }
}

#[cfg(test)]
mod mapping_tests {
    use super::tests::lock_guards;
    use super::{PAGE, by_range, title_memory_reading, title_memory_remapping};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    /// Addresses only these tests hold (nothing is read there).
    const HERE: usize = 0x7e00_0000_0000;

    /// A change of mapping waits for a read in place of its bytes that is
    /// under way, and such a read waits for the change.
    #[test]
    fn reads_in_place_and_remappings_wait_for_each_other() {
        // Holds the mapping lock for a while: no other test that reads
        // title memory in place (the texture refreshes) runs meanwhile.
        let _one = lock_guards();
        // A reader holds; the change does not begin until it lets go.
        let changed = Arc::new(AtomicBool::new(false));
        let reading = title_memory_reading(HERE + PAGE / 2, PAGE);
        let (began, wait) = mpsc::channel();
        let change = {
            let changed = changed.clone();
            std::thread::spawn(move || {
                began.send(()).unwrap();
                let _changing = title_memory_remapping(HERE + PAGE, PAGE);
                changed.store(true, Ordering::SeqCst);
            })
        };
        wait.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!changed.load(Ordering::SeqCst), "the mapping changed under a read in place");
        drop(reading);
        change.join().unwrap();
        assert!(changed.load(Ordering::SeqCst));
        // A change holds; the read does not begin until it is done.
        let read = Arc::new(AtomicBool::new(false));
        let changing = title_memory_remapping(HERE, 2 * PAGE);
        let (began, wait) = mpsc::channel();
        let reader = {
            let read = read.clone();
            std::thread::spawn(move || {
                began.send(()).unwrap();
                let _reading = title_memory_reading(HERE + PAGE + 16, 16);
                read.store(true, Ordering::SeqCst);
            })
        };
        wait.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!read.load(Ordering::SeqCst), "a read in place began during a change of mapping");
        drop(changing);
        reader.join().unwrap();
        assert!(read.load(Ordering::SeqCst));
    }

    /// Reads and changes whose bytes do not meet do not wait for each other,
    /// and a change waiting for a read keeps new reads of its bytes out (but
    /// not of others).
    #[test]
    fn reads_and_remappings_of_other_bytes_go_on() {
        if !by_range() {
            return; // VICTORY_MAPPING_RANGES=0: one lock over all of it
        }
        let at = HERE + 16 * PAGE;
        let reading = title_memory_reading(at, PAGE);
        // Another page changes while the read is under way, and another
        // read goes on while that change is.
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let changing = title_memory_remapping(at + PAGE, PAGE);
            let (read, was_read) = mpsc::channel();
            std::thread::spawn(move || {
                let _reading = title_memory_reading(at + 2 * PAGE, PAGE);
                read.send(()).unwrap();
            });
            let other = was_read.recv_timeout(Duration::from_secs(10)).is_ok();
            drop(changing);
            done.send(other).unwrap();
        });
        assert_eq!(finished.recv_timeout(Duration::from_secs(10)), Ok(true), "kept apart though their bytes do not meet");
        // A change of the read's page waits, and keeps a new read of its
        // bytes out meanwhile; a read elsewhere is not kept out.
        let changed = Arc::new(AtomicBool::new(false));
        let change = {
            let changed = changed.clone();
            std::thread::spawn(move || {
                let _changing = title_memory_remapping(at, PAGE);
                changed.store(true, Ordering::SeqCst);
            })
        };
        while !super::spans().changes.iter().any(|c| c.1 == at) {
            std::thread::yield_now();
        }
        assert!(super::title_memory_reading_now(at + 8, 8).is_none(), "a read began over a change that waits");
        assert!(super::title_memory_reading_now(at + 3 * PAGE, 8).is_some());
        std::thread::sleep(Duration::from_millis(50));
        assert!(!changed.load(Ordering::SeqCst), "the mapping changed under a read in place");
        drop(reading);
        change.join().unwrap();
        assert!(changed.load(Ordering::SeqCst));
    }
}

#[cfg(test)]
mod blocks_tests {
    use super::Blocks;
    use std::collections::BTreeSet;

    /// The bit set answers as a tree of blocks does, across word ends:
    /// membership of whole ranges, and the blocks of a range in order.
    #[test]
    fn blocks_answer_as_a_tree_of_blocks() {
        let mut bits = Blocks::new();
        let mut tree = BTreeSet::new();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |most: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % most
        };
        for step in 0..4000 {
            let (at, len) = (1000 + next(400), next(140));
            match step % 4 {
                0 => {
                    bits.extend(at..at + len);
                    tree.extend(at..at + len);
                }
                1 => {
                    bits.insert(at);
                    tree.insert(at);
                }
                _ => {
                    bits.remove(at);
                    tree.remove(&at);
                }
            }
            let (from, n) = (990 + next(420), next(200));
            let range = from..from + n;
            assert_eq!(
                bits.holds(range.clone()),
                tree.range(range.clone()).count() == n,
                "all of {range:?} at step {step}"
            );
            assert_eq!(
                bits.range(range.clone()).collect::<Vec<_>>(),
                tree.range(range.clone()).copied().collect::<Vec<_>>(),
                "the blocks of {range:?} at step {step}"
            );
        }
    }
}

#[cfg(test)]
mod wrote_tests {
    use super::tests::{fresh_watched, lock_guards};
    use super::{PAGE, ask_once, ask_once_end, asked, writes, wrote};

    #[test]
    fn wrote_keeps_ahead_asked_at_or_above_written_timestamps() {
        // Shares the journal and the askings with the guard tests: one at a time.
        let _one = lock_guards();
        use windows_sys::Win32::System::Memory::{MEM_RELEASE, VirtualFree};
        let p = fresh_watched(PAGE);
        assert_ne!(p, 0);

        ask_once(&[(p, p + PAGE)]);
        wrote(p, PAGE);

        let (now, written) = writes(p, p + PAGE, false).expect("write-watched page");
        let (asked_now, asked_written) = asked(p, p + PAGE).unwrap_or_else(|| {
            let j = super::JOURNAL.read().unwrap_or_else(|e| e.into_inner());
            panic!(
                "not in ask_once range: known_view={} kept={} ahead={:?} page={}",
                super::known_view(p),
                j.kept,
                j.ahead,
                p / PAGE
            )
        });
        ask_once_end();

        // SAFETY: freeing our private reservation.
        unsafe { VirtualFree(p as *mut _, 0, MEM_RELEASE) };

        assert_eq!(now, asked_now);
        let page = p / PAGE;
        let &(_, w) = written
            .iter()
            .find(|&&(pg, _)| pg == page)
            .expect("page recorded by wrote()");
        assert_eq!(asked_written, written);
        assert!(
            now >= w,
            "writes() returned now={now} < written timestamp w={w} after wrote()"
        );
    }
}
