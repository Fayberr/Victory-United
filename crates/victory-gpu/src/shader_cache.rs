<<<<<<< HEAD
//! Translated shaders kept on disk (decision 0003): offline warm-up and title
//! execution share the same shader cache, under the configured cache folder.
//! A key includes the console container, translator sources (the BLAKE3 of
//! them) and configuration (its source fingerprint and settings, see `build`),
//! and the host wave width and descriptor-heap limit used by the emitter, so a
//! changed translator or host never serves an old translation. Entries go
//! through `victory-pipeline`'s content-addressed cache, which writes them
//! atomically; an entry that does not read back is translated again.
//! Translated DXIL and deterministic unsupported results are persisted.
//! Runtime pipelines can use the translation even when the cache cannot store
//! it.
||||||| 85d564167
//! Translated shaders kept on disk (decision 0003): under the configured
//! cache folder, keyed by the container's content and by the translator
//! (the BLAKE3 of its sources) and its settings, so a changed translator
//! never serves an old translation. Entries go through
//! `victory-pipeline`'s content-addressed cache, which writes them
//! atomically; an entry that does not read back is translated again.
=======
//! Translated shaders kept on disk (decision 0003): under the configured
//! cache folder, keyed by the container's content, by the translator's
//! version ([`victory_shader::TRANSLATOR_VERSION`]) and by its settings.
//! A rebuild that leaves the version keeps every translation: the store
//! grows by the shaders a change made translate, not by a whole new set
//! each build (16 GB of sets of earlier builds had piled up when the key
//! was the translator's source). Entries go through `victory-pipeline`'s
//! content-addressed cache, which writes them atomically; an entry that
//! does not read back is translated again.
//!
//! Each entry also names the build of the translator that made it (the
//! BLAKE3 of its sources, `build.rs`). One of an earlier build is served
//! as it is, and counted; `victory shaders` ([`precompile`]) translates it
//! again, keeps the new one and says how many came out different (then
//! the version should have been bumped). `VICTORY_SHADER_FRESH=1` does the
//! same in a run; `VICTORY_SHADER_KEY=source` keys by the translator's
//! source as before (a separate set for each build).
>>>>>>> avatar/main
//!
//! The entry is our own encoding of [`HostShader`]: a field added to it
//! must be added to `encode` and `decode` too, and [`FORMAT`] bumped.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

<<<<<<< HEAD
use victory_pipeline::{Cache, CacheKey};
||||||| 85d564167
use victory_pipeline::{Cache, Converter};
=======
/// The encoding of entries ([`encode`]), part of the key.
const FORMAT: u32 = 2;

use victory_pipeline::{Cache, Converter};
>>>>>>> avatar/main
use victory_shader::emit::{HostShader, StaticSampler, UserData};
use victory_shader::resource::RootIndexedView;
use victory_shader::{HwStage, RootType, ShaderError};

static CACHE: OnceLock<Cache> = OnceLock::new();
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);
/// Hits on entries an earlier build of the translator made (served as they
/// are, or when refreshing translated again); of those translated again,
/// how many came out different.
static STALE: AtomicU64 = AtomicU64::new(0);
static REFRESHED: AtomicU64 = AtomicU64::new(0);
static CHANGED: AtomicU64 = AtomicU64::new(0);

/// The build of the translator (the BLAKE3 of its sources, `build.rs`).
const SOURCE: &str = env!("VICTORY_TRANSLATOR");

/// How cached translations are keyed (`VICTORY_SHADER_KEY`).
fn keyed_by_source() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VICTORY_SHADER_KEY").is_ok_and(|v| v == "source"))
}

/// Whether a run translates entries of an earlier build again
/// (`VICTORY_SHADER_FRESH=1`; [`precompile`] always does).
fn refresh_in_runs() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_SHADER_FRESH").is_some_and(|v| v == "1"))
}

/// Keeps translated shaders in `dir` from now on.
pub fn use_shader_cache(dir: PathBuf) -> bool {
    match Cache::open(dir) {
        Ok(cache) => CACHE.set(cache).is_ok(),
        Err(e) => {
            tracing::warn!(error = %e, "no shader cache");
            false
        }
    }
}

/// Select the same hardware and shader capabilities that a title run uses.
pub fn prepare_host() -> bool {
    if crate::host::host().is_none() {
        return false;
    }
    crate::descriptor::host::probe_view_limit();
    true
}

/// The result of looking up or translating one console shader.
pub enum PrecompileOutcome {
    Cached,
    Translated,
    CachedRefusal,
    Refused,
}

/// Fills the runtime shader cache and reports whether the entry was already there.
pub fn precompile_shader(container: &[u8]) -> Result<PrecompileOutcome, String> {
    let cache = CACHE.get().ok_or("no shader cache")?;
    let (result, status) = lookup(cache, container);
    match (result, status) {
        (_, CacheStatus::Failed(error)) => Err(error),
        (Ok(_), CacheStatus::Hit) => Ok(PrecompileOutcome::Cached),
        (Ok(_), CacheStatus::Stored) => Ok(PrecompileOutcome::Translated),
        (Err(_), CacheStatus::Hit) => Ok(PrecompileOutcome::CachedRefusal),
        (Err(_), _) => Ok(PrecompileOutcome::Refused),
        (Ok(_), CacheStatus::Unstored) => Err("translation was not stored".into()),
    }
}

/// Successful shader translations found on disk and generated since startup.
pub(crate) fn counts() -> (u64, u64) {
    (HITS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed))
}

/// Since the start: entries of an earlier build of the translator met,
/// those of them translated again, and those that came out different.
pub fn stale_counts() -> (u64, u64, u64) {
    (STALE.load(Ordering::Relaxed), REFRESHED.load(Ordering::Relaxed), CHANGED.load(Ordering::Relaxed))
}

/// What identifies a translation beside its container: the translator's
/// version (or with `VICTORY_SHADER_KEY=source` its sources), the form of
/// entries, the settings it runs with, and what it was told of the host.
/// Not our executable: a rebuild of anything else used to translate every
/// shader again.
fn build() -> [u8; 32] {
    // The view heap's null slot differs by host GPU: a translation cached
    // without the clamp must not be served (`host_settings` below).
    crate::descriptor::host::probe_view_limit();
    static FIXED: OnceLock<Vec<u8>> = OnceLock::new();
    let fixed = FIXED.get_or_init(|| {
        let limit = victory_shader::emit::loop_limit();
        if limit != victory_shader::emit::DEFAULT_LOOP_LIMIT {
            tracing::warn!(
                limit,
                "VICTORY_SHADER_LOOP_LIMIT: translated loops stop sooner"
            );
        }
        let mut bytes = if keyed_by_source() {
            SOURCE.as_bytes().to_vec()
        } else {
            format!("version {}", victory_shader::TRANSLATOR_VERSION).into_bytes()
        };
        bytes.extend_from_slice(&FORMAT.to_le_bytes());
        bytes.extend_from_slice(&limit.to_le_bytes());
        // Which group barriers wait is part of the translation too.
        bytes.push(victory_shader::emit::group_sync());
        // So are the switches that pick the form of a wave64 program:
        // without them here a run with `VICTORY_WAVE_WIDE=0` was
        // served the translations an earlier run of the same
        // executable had cached with the switch unset.
        for name in [
            "VICTORY_WAVE_WIDE",
            "VICTORY_WAVE_DEEP_SKIPS",
            "VICTORY_WAVE_OWN_BITS",
            "VICTORY_WAVE_LOCAL",
            "VICTORY_LDS_FLAGS",
            "VICTORY_F16_NEAREST",
            "VICTORY_HELPER_OWN",
            "VICTORY_FIRSTLANE_OWN",
            "VICTORY_FIRST_HALF",
            "VICTORY_STRICT_JOINS",
            "VICTORY_MESH_OUTPUT_STRICT",
            "VICTORY_BVH_FENCES",
            "VICTORY_BVH_BOUNDS",
        ] {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(b'=');
            if let Some(value) = std::env::var_os(name) {
                bytes.extend_from_slice(value.as_encoded_bytes());
            }
            bytes.push(0);
        }
        // And which clean-up passes ran over the translated code.
        bytes.extend_from_slice(&victory_shader::emit::shader_opt().to_le_bytes());
        // Renderer port 5 (`VICTORY_PORT_CBV=1`): constant buffers read as raw
        // bytes. Only when on, so the translations cached without it keep
        // their key.
        if victory_shader::emit::constants_raw() {
            bytes.extend_from_slice(b"VICTORY_PORT_CBV=1");
        }
        bytes
    });
    let mut hash = blake3::Hasher::new();
    hash.update(fixed);
    for value in victory_shader::emit::host_settings() {
        hash.update(&value.to_le_bytes());
    }
    *hash.finalize().as_bytes()
}

/// What tells one set of translations from another ([`build`]).
pub(crate) fn translation() -> [u8; 32] {
    build()
}

/// The offline warm-up (`victory translate --warm`): `container` is
/// translated into the cache unless it is there already. `Ok(true)`: it
/// was translated now. Needs [`use_shader_cache`] and the same host facts
/// the run will have (`victory_shader::emit::translation_fingerprint`).
pub fn warm(container: &[u8]) -> Result<bool, String> {
    let cache = CACHE.get().ok_or("no shader cache")?;
    let (result, status) = lookup(cache, container);
    match status {
        CacheStatus::Failed(error) => Err(error),
        _ => result.map(|_| status != CacheStatus::Hit),
    }
}

/// Puts the translation of `container` into the cache before any title asks
/// for it (`victory shaders`): whether it was there already, or why it does
/// not translate. The host is opened first, as a run does: what the card
/// can do decides the form of some programs.
pub fn precompile(container: &[u8]) -> Result<bool, String> {
    let _ = crate::host::host();
    let misses = MISSES.load(Ordering::Relaxed);
    let translate = |c: &[u8]| victory_shader::emit::translate(c).map_err(|e| e.to_string());
    match CACHE.get() {
        Some(cache) => cached(cache, &build(), container, translate, true)?,
        None => translate(container)?,
    };
    // (Counts other threads' misses too: the caller only adds them up.)
    Ok(MISSES.load(Ordering::Relaxed) == misses)
}

/// The translation of a console shader container, from the cache when
/// there is one.
pub(crate) fn host_shader(container: &[u8]) -> Result<HostShader, String> {
<<<<<<< HEAD
    host_shader_cached(container).map(|(shader, _)| shader)
}

/// Like [`host_shader`], also reporting whether the translation came from the
/// disk cache (`true`) or was freshly translated (`false`).
pub(crate) fn host_shader_cached(container: &[u8]) -> Result<(HostShader, bool), String> {
    if let Some(s) = stand_in(container) {
        return Ok((s, false));
    }
    let (result, status) = match CACHE.get() {
        Some(cache) => lookup(cache, container),
        None => (
            victory_shader::emit::translate(container).map_err(|e| e.to_string()),
            CacheStatus::Unstored,
        ),
    };
    if let CacheStatus::Failed(error) = &status {
        static WARNED: OnceLock<()> = OnceLock::new();
        WARNED.get_or_init(|| tracing::warn!(%error, "shader cache unavailable for an entry"));
    }
    if result.is_ok() {
        let counter = if status == CacheStatus::Hit {
            &HITS
        } else {
            &MISSES
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
    let shader = match result {
        Err(e) => exposure_stub(container).ok_or(e)?,
        Ok(shader) => shader,
    };
    validate_runtime(&shader.dxil)?;
    Ok((shader, status == CacheStatus::Hit))
}

/// Optional stand-ins from `victory_shader::emit::stand_in`:
/// - `VICTORY_AMBIENT_STUB=1`: replaces diffuse GI temporal resolve passes with
///   a constant ambient light floor (`VICTORY_AMBIENT_STUB_VALUE`, default 0.1).
/// - `VICTORY_SHADOWMASK_STUB=1`: replaces RT distant shadow passes with 1.0
///   (lit) (`VICTORY_SHADOWMASK_STUB_VALUE`, default 1.0).
fn stand_in(container: &[u8]) -> Option<HostShader> {
    use victory_shader::emit::stand_in as s;
    let hash = blake3::hash(container).to_hex()[..16].to_owned();
    let kinds = [
        ("VICTORY_AMBIENT_STUB", s::AMBIENT_HASHES, s::AMBIENT_VALUE, s::ambient_slots(&hash)),
        ("VICTORY_SHADOWMASK_STUB", s::SHADOWMASK_HASHES, s::SHADOWMASK_VALUE, s::SHADOWMASK_SLOTS),
    ];
    for (name, hashes, value, defaults) in kinds {
        if std::env::var(name).ok().as_deref().map(str::trim) != Some("1") {
            continue;
        }
        let listed = match std::env::var(format!("{name}_HASHES")) {
            Ok(v) => v
                .split(|c: char| c == ',' || c.is_whitespace())
                .any(|h| h.eq_ignore_ascii_case(&hash)),
            Err(_) => hashes.contains(&hash.as_str()),
        };
        if !listed {
            continue;
        }
        let value = std::env::var(format!("{name}_VALUE"))
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(value);
        let slots = std::env::var(format!("{name}_SLOTS")).unwrap_or_default();
        let plan = s::Plan::new(value)
            .with_overrides(defaults)
            .with_overrides(&slots);
        match s::storing_plan(container, plan) {
            Ok((shader, plan)) => {
                tracing::warn!(
                    %hash,
                    stand_in = name,
                    value,
                    outputs = ?plan.seen,
                    "pass replaced by a stand-in writing each output in its own encoding"
                );
                return Some(shader);
            }
            Err(e) => {
                tracing::warn!(%hash, stand_in = name, error = %e, "stand-in does not translate");
            }
        }
    }
    None
}

/// Optional validation of translated title shaders, including cache hits.
/// Normal runs do not load the validator or change the translation.
fn validate_runtime(dxil: &[u8]) -> Result<(), String> {
    use victory_shader::emit::validator::Validator;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static VALIDATOR: OnceLock<Result<std::sync::Mutex<Validator>, String>> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("VICTORY_VALIDATE_DXIL").is_some_and(|v| v == "1")) {
        return Ok(());
    }
    let validator = VALIDATOR.get_or_init(|| {
        Validator::load(Validator::default_path().as_deref()).map(std::sync::Mutex::new)
    });
    let validator = validator.as_ref().map_err(|e| format!("DXIL validator unavailable: {e}"))?;
    // Validate a private copy: the validator signs its input in place.
    let mut bytes = dxil.to_vec();
    validator.lock().unwrap_or_else(|e| e.into_inner())
        .validate(&mut bytes).map_err(|e| format!("DXIL validation rejected shader: {e}"))
}

/// `VICTORY_EXPOSURE_STUB=1`: replaces untranslatable auto-exposure shader
/// with a constant mid-grey luminance stand-in.
fn exposure_stub(container: &[u8]) -> Option<HostShader> {
    use victory_shader::emit::exposure;
    if std::env::var("VICTORY_EXPOSURE_STUB").ok()?.trim() != "1" {
        return None;
    }
    let hash = blake3::hash(container).to_hex()[..16].to_owned();
    let listed = match std::env::var("VICTORY_EXPOSURE_STUB_HASHES") {
        Ok(v) => v
            .split(|c: char| c == ',' || c.is_whitespace())
            .any(|h| h.eq_ignore_ascii_case(&hash)),
        Err(_) => exposure::HASHES.contains(&hash.as_str()),
    };
    if !listed {
        return None;
    }
    let value = std::env::var("VICTORY_EXPOSURE_STUB_VALUE")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(exposure::VALUE);
    match exposure::stub(container, value) {
        Ok(s) => {
            tracing::warn!(%hash, value, "exposure shader replaced by a constant stand-in");
            Some(s)
        }
        Err(e) => {
            tracing::warn!(%hash, error = %e, "exposure stand-in does not translate either");
            None
        }
||||||| 85d564167
    let translate = |c: &[u8]| victory_shader::emit::translate(c).map_err(|e| e.to_string());
    match CACHE.get() {
        Some(cache) => cached(cache, &build(), container, translate),
        None => translate(container),
=======
    let translate = |c: &[u8]| victory_shader::emit::translate(c).map_err(|e| e.to_string());
    match CACHE.get() {
        Some(cache) => cached(cache, &build(), container, translate, refresh_in_runs()),
        None => translate(container),
>>>>>>> avatar/main
    }
}

<<<<<<< HEAD
fn target() -> [u32; 2] {
    [
        victory_shader::emit::host_max_wave(),
        victory_shader::emit::view_heap_limit(),
    ]
||||||| 85d564167
/// The cache entry's converter: the build and the container in, the
/// encoded translation out. `ran` tells a miss from a hit.
struct Translate<F> {
    translate: F,
    ran: AtomicBool,
=======
/// The cache entry's converter: the build and the container in, the
/// encoded translation out. `ran` tells a miss from a hit.
struct Translate<'a, F> {
    translate: F,
    /// The build of the translator, written into the entry.
    source: &'a str,
    ran: AtomicBool,
>>>>>>> avatar/main
}

<<<<<<< HEAD
/// DXBC's declared size excludes trailing allocation padding. Keep the title's
/// original bytes for pipeline identity, but use this slice for translation.
fn canonical_container(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(b"DXBC")
        && let Some(size) = bytes.get(0x18..0x1c)
    {
        let size = u32::from_le_bytes(size.try_into().expect("four bytes")) as usize;
        if size >= 0x20
            && let Some(container) = bytes.get(..size)
        {
            return container;
        }
||||||| 85d564167
impl<F: Fn(&[u8]) -> Result<HostShader, String> + Send + Sync> Converter for Translate<F> {
    fn id(&self) -> &'static str {
        "shader.dxil"
=======
impl<F: Fn(&[u8]) -> Result<HostShader, String> + Send + Sync> Converter for Translate<'_, F> {
    fn id(&self) -> &'static str {
        "shader.dxil"
>>>>>>> avatar/main
    }
<<<<<<< HEAD
    bytes
}

#[derive(Debug, PartialEq, Eq)]
enum CacheStatus {
    Hit,
    Stored,
    Unstored,
    Failed(String),
}

type Translation = Result<HostShader, String>;

fn lookup(cache: &Cache, container: &[u8]) -> (Translation, CacheStatus) {
    let container = canonical_container(container);
    for _ in 0..2 {
        let expected = target();
        let translated = cached(
            cache,
            &build(),
            expected,
            container,
            victory_shader::emit::translate,
            || target() == expected,
        );
        if target() == expected {
            return translated;
        }
||||||| 85d564167
    fn version(&self) -> u32 {
        // The build fingerprint in the input does the versioning.
        1
    }
    fn convert(&self, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.ran.store(true, Ordering::Relaxed);
        let shader = (self.translate)(&input[32..]).map_err(anyhow::Error::msg)?;
        Ok(encode(&shader))
=======
    fn version(&self) -> u32 {
        // The build fingerprint in the input does the versioning.
        1
    }
    fn convert(&self, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.ran.store(true, Ordering::Relaxed);
        let shader = (self.translate)(&input[32..]).map_err(anyhow::Error::msg)?;
        Ok(encode(&shader, self.source))
>>>>>>> avatar/main
    }
    let error = "host shader capabilities changed during translation".to_owned();
    (Err(error.clone()), CacheStatus::Failed(error))
}

fn shader_key(build: &[u8; 32], [wave, view_limit]: [u32; 2], container: &[u8]) -> CacheKey {
    // Bump the format version if the serialized HostShader or refusal changes.
    CacheKey::from_parts(
        "shader.dxil",
        2,
        &[
            build,
            &wave.to_le_bytes(),
            &view_limit.to_le_bytes(),
            container,
        ],
    )
}

fn translate(container: &[u8]) -> Result<HostShader, String> {
    victory_shader::emit::translate(container).map_err(|e| e.to_string())
}

/// `refresh`: an entry an earlier build of the translator made is
/// translated again (and replaced) rather than served.
fn cached(
    cache: &Cache,
    build: &[u8; 32],
    target: [u32; 2],
    container: &[u8],
<<<<<<< HEAD
    translate: impl FnOnce(&[u8]) -> Result<HostShader, ShaderError>,
    still_current: impl FnOnce() -> bool,
) -> (Translation, CacheStatus) {
    let key = shader_key(build, target, container);
    let path = cache.path_for(&key);
    let read = || {
        std::fs::read(&path)
            .ok()
            .and_then(|bytes| decode_record(&bytes))
||||||| 85d564167
    translate: impl Fn(&[u8]) -> Result<HostShader, String> + Send + Sync,
) -> Result<HostShader, String> {
    let input = [&build[..], container].concat();
    let converter = Translate {
        translate,
        ran: AtomicBool::new(false),
=======
    translate: impl Fn(&[u8]) -> Result<HostShader, String> + Send + Sync,
    refresh: bool,
) -> Result<HostShader, String> {
    cached_by(cache, build, SOURCE, container, translate, refresh)
}

/// [`cached`] with the build of the translator given (`source`).
fn cached_by(
    cache: &Cache,
    build: &[u8; 32],
    source: &str,
    container: &[u8],
    translate: impl Fn(&[u8]) -> Result<HostShader, String> + Send + Sync,
    refresh: bool,
) -> Result<HostShader, String> {
    let input = [&build[..], container].concat();
    let converter = Translate {
        translate,
        source,
        ran: AtomicBool::new(false),
>>>>>>> avatar/main
    };
<<<<<<< HEAD
    if let Some(entry) = read() {
        return (entry, CacheStatus::Hit);
||||||| 85d564167
    for _ in 0..2 {
        let path = cache
            .get_or_convert(&converter, &input)
            .map_err(|e| format!("{e:#}"))?;
        if let Some(shader) = std::fs::read(&path).ok().and_then(|b| decode(&b)) {
            let (counter, what) = if converter.ran.load(Ordering::Relaxed) {
                (&MISSES, "translated and cached")
            } else {
                (&HITS, "from the cache")
            };
            counter.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(path = %path.display(), "shader {what}");
            return Ok(shader);
        }
        tracing::debug!(path = %path.display(), "unreadable shader cache entry; translating again");
        let _ = std::fs::remove_file(&path);
=======
    let mut old: Option<HostShader> = None;
    for _ in 0..3 {
        let path = cache
            .get_or_convert(&converter, &input)
            .map_err(|e| format!("{e:#}"))?;
        if let Some((shader, made_by)) = std::fs::read(&path).ok().and_then(|b| decode(&b)) {
            let ran = converter.ran.load(Ordering::Relaxed);
            if !ran && made_by != source.as_bytes() {
                STALE.fetch_add(1, Ordering::Relaxed);
                if refresh && old.is_none() {
                    // Translated again in its place; what came out before
                    // is what the new one is compared with.
                    tracing::debug!(path = %path.display(), "shader cache entry of an earlier translator; translating again");
                    old = Some(shader);
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                static SAID: AtomicBool = AtomicBool::new(false);
                if !SAID.swap(true, Ordering::Relaxed) {
                    tracing::info!("shader translations of an earlier build of the translator (same version) are served; `victory shaders` translates them again");
                }
            }
            if let Some(old) = old.take() {
                REFRESHED.fetch_add(1, Ordering::Relaxed);
                if old != shader {
                    CHANGED.fetch_add(1, Ordering::Relaxed);
                }
            }
            let (counter, what) = if ran {
                (&MISSES, "translated and cached")
            } else {
                (&HITS, "from the cache")
            };
            counter.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(path = %path.display(), "shader {what}");
            return Ok(shader);
        }
        tracing::debug!(path = %path.display(), "unreadable shader cache entry; translating again");
        let _ = std::fs::remove_file(&path);
>>>>>>> avatar/main
    }
    let translated = translate(container);
    let persist = matches!(translated, Ok(_) | Err(ShaderError::Unsupported { .. }));
    let result = translated.map_err(|e| e.to_string());
    if !persist || !still_current() {
        return (result, CacheStatus::Unstored);
    }
    let status = match cache.write(&key, &encode_record(&result)) {
        Ok(()) => CacheStatus::Stored,
        Err(_) if read().is_some() => CacheStatus::Hit,
        Err(e) => CacheStatus::Failed(format!("{}: {e:#}", path.display())),
    };
    (result, status)
}

<<<<<<< HEAD
const REFUSED: &[u8; 4] = b"VSHE";

fn encode_record(result: &Translation) -> Vec<u8> {
    let payload = match result {
        Ok(shader) => encode(shader),
        Err(reason) => [REFUSED.as_slice(), reason.as_bytes()].concat(),
    };
    let digest = blake3::hash(&payload);
    let mut record = Vec::with_capacity(digest.as_bytes().len() + payload.len());
    record.extend_from_slice(digest.as_bytes());
    record.extend_from_slice(&payload);
    record
}

fn decode_record(bytes: &[u8]) -> Option<Translation> {
    let (digest, payload) = bytes.split_at_checked(32)?;
    if blake3::hash(payload).as_bytes() != digest {
        return None;
    }
    if let Some(reason) = payload.strip_prefix(REFUSED) {
        return Some(Err(std::str::from_utf8(reason).ok()?.to_owned()));
    }
    decode(payload).map(Ok)
}

const MAGIC: &[u8; 4] = b"VSH3";
||||||| 85d564167
const MAGIC: &[u8; 4] = b"VSH1";
=======
const MAGIC: &[u8; 4] = b"VSH2";
>>>>>>> avatar/main
const STAGES: [HwStage; 8] = [
    HwStage::Ls,
    HwStage::Hs,
    HwStage::Es,
    HwStage::Gs,
    HwStage::Vs,
    HwStage::Ps,
    HwStage::Ms,
    HwStage::Cs,
];

fn root_code(r: RootType) -> u32 {
    match r {
        RootType::Constants => 0,
        RootType::Descriptor => 1,
        RootType::Table => 2,
        RootType::ImmediateSampler => 3,
        RootType::ResourceHeap => 4,
        RootType::SamplerHeap => 5,
        RootType::Other(x) => 6 + x,
    }
}

fn root_type(code: u32) -> RootType {
    match code {
        0 => RootType::Constants,
        1 => RootType::Descriptor,
        2 => RootType::Table,
        3 => RootType::ImmediateSampler,
        4 => RootType::ResourceHeap,
        5 => RootType::SamplerHeap,
        x => RootType::Other(x - 6),
    }
}

/// Little-endian words and length-prefixed lists, after the build of the
/// translator that made the entry (`source`, length-prefixed).
fn encode(s: &HostShader, source: &str) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    let word = |out: &mut Vec<u8>, v: u32| out.extend(v.to_le_bytes());
    word(&mut out, source.len() as u32);
    out.extend(source.as_bytes());
    let stage = STAGES.iter().position(|&x| x == s.stage).unwrap_or(0) as u32;
    word(&mut out, stage);
    word(&mut out, s.dxil.len() as u32);
    out.extend(&s.dxil);
    word(&mut out, s.user_data.len() as u32);
    for u in &s.user_data {
        for v in [u.index, u32::from(u.slot), root_code(u.root), u.dword] {
            word(&mut out, v);
        }
    }
    word(&mut out, s.samplers.len() as u32);
    for sampler in &s.samplers {
        for &v in sampler {
            word(&mut out, v);
        }
    }
    word(&mut out, s.static_samplers.len() as u32);
    for t in &s.static_samplers {
        let [u, v, w] = t.address;
        let words = [
            t.filter,
            u,
            v,
            w,
            t.mip_lod_bias.to_bits(),
            t.max_anisotropy,
            t.comparison,
            t.border,
            t.min_lod.to_bits(),
            t.max_lod.to_bits(),
        ];
        for v in words {
            word(&mut out, v);
        }
    }
    for slots in [&s.root_cbvs, &s.root_srvs, &s.root_uavs, &s.written_root_uavs] {
        word(&mut out, slots.len() as u32);
        out.extend(slots);
    }
    word(&mut out, u32::from(s.writes_guest_memory));
    word(&mut out, s.root_indexed_views.len() as u32);
    for view in &s.root_indexed_views {
        for value in [
            u32::from(view.table_slot),
            u32::from(view.constant_slot),
            view.constant_dword,
            view.byte_offset,
            u32::from(view.shift),
        ] {
            word(&mut out, value);
        }
    }
    word(&mut out, s.half_views.len() as u32);
    for &(slot, dword) in &s.half_views {
        word(&mut out, u32::from(slot));
        word(&mut out, dword);
    }
    out
}

/// The shader and the build of the translator that made it.
fn decode(bytes: &[u8]) -> Option<(HostShader, Vec<u8>)> {
    struct Reader<'a>(&'a [u8]);
    impl<'a> Reader<'a> {
        fn take(&mut self, n: usize) -> Option<&'a [u8]> {
            let (head, rest) = self.0.split_at_checked(n)?;
            self.0 = rest;
            Some(head)
        }
        fn word(&mut self) -> Option<u32> {
            Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
        }
        fn list(&mut self) -> Option<Vec<u8>> {
            let n = self.word()? as usize;
            Some(self.take(n)?.to_vec())
        }
    }
    let mut r = Reader(bytes);
    if r.take(4)? != MAGIC {
        return None;
    }
    let made_by = r.list()?;
    let stage = *STAGES.get(r.word()? as usize)?;
    let dxil = r.list()?;
    let n = r.word()? as usize;
    let mut user_data = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        user_data.push(UserData {
            index: r.word()?,
            slot: u8::try_from(r.word()?).ok()?,
            root: root_type(r.word()?),
            dword: r.word()?,
        });
    }
    let n = r.word()? as usize;
    let mut samplers = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        samplers.push([r.word()?, r.word()?, r.word()?, r.word()?]);
    }
    let n = r.word()? as usize;
    let mut static_samplers = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        static_samplers.push(StaticSampler {
            filter: r.word()?,
            address: [r.word()?, r.word()?, r.word()?],
            mip_lod_bias: f32::from_bits(r.word()?),
            max_anisotropy: r.word()?,
            comparison: r.word()?,
            border: r.word()?,
            min_lod: f32::from_bits(r.word()?),
            max_lod: f32::from_bits(r.word()?),
        });
    }
    let root_cbvs = r.list()?;
    let root_srvs = r.list()?;
    let root_uavs = r.list()?;
    let written_root_uavs = r.list()?;
    let writes_guest_memory = match r.word()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let n = r.word()? as usize;
    let mut root_indexed_views = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        root_indexed_views.push(RootIndexedView {
            table_slot: u8::try_from(r.word()?).ok()?,
            constant_slot: u8::try_from(r.word()?).ok()?,
            constant_dword: r.word()?,
            byte_offset: r.word()?,
            shift: u8::try_from(r.word()?).ok().filter(|&s| s < 32)?,
        });
    }
<<<<<<< HEAD
    let n = r.word()? as usize;
    let mut half_views = Vec::with_capacity(n.min(256));
    for _ in 0..n {
        half_views.push((u8::try_from(r.word()?).ok()?, r.word()?));
    }
    r.0.is_empty().then_some(HostShader {
||||||| 85d564167
    r.0.is_empty().then_some(HostShader {
=======
    r.0.is_empty().then_some((
        HostShader {
>>>>>>> avatar/main
        stage,
        dxil,
        user_data,
        samplers,
        static_samplers,
        root_cbvs,
        root_srvs,
        root_uavs,
        written_root_uavs,
        writes_guest_memory,
        root_indexed_views,
<<<<<<< HEAD
        half_views,
    })
||||||| 85d564167
    })
=======
        },
        made_by,
    ))
>>>>>>> avatar/main
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shader() -> HostShader {
        HostShader {
            stage: HwStage::Ps,
            dxil: b"DXBC and the rest".to_vec(),
            user_data: vec![UserData {
                index: 8,
                slot: 1,
                root: RootType::Table,
                dword: 0,
            }],
            samplers: vec![[0x400, 0xc0_0000, 0x18f0_0000, 0]],
            static_samplers: vec![StaticSampler::from_words([
                0x400,
                0xc0_0000,
                0x18f0_0000,
                0,
            ])],
            root_cbvs: vec![2],
            root_srvs: vec![0, 3],
            root_uavs: vec![1, 2],
            written_root_uavs: vec![2],
            writes_guest_memory: true,
            root_indexed_views: vec![RootIndexedView {
                table_slot: 1,
                constant_slot: 2,
                constant_dword: 3,
                byte_offset: 32,
                shift: 5,
            }],
            half_views: vec![(0, 8), (4, 0)],
        }
    }

    #[test]
    fn translations_come_back_from_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let build = [7u8; 32];
        let target = [32, 999_999];
        let container = b"a console container".as_slice();
<<<<<<< HEAD
        let translate = |_: &[u8]| Ok(shader());
        let entry = || cached(&cache, &build, target, container, translate, || true);
        assert_eq!(entry(), (Ok(shader()), CacheStatus::Stored));
        assert_eq!(entry(), (Ok(shader()), CacheStatus::Hit));
        for other_target in [[64, target[1]], [target[0], 0]] {
            assert_eq!(
                cached(&cache, &build, other_target, container, translate, || true),
                (Ok(shader()), CacheStatus::Stored)
            );
||||||| 85d564167
        let runs = AtomicU64::new(0);
        let translate = |_: &[u8]| {
            runs.fetch_add(1, Ordering::Relaxed);
            Ok(shader())
        };
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 1, "the second came from disk");

        // Another build translates again.
        assert_eq!(cached(&cache, &[8; 32], container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 2);

        // A damaged entry is a miss, translated and stored again.
        for e in walk(dir.path()) {
            std::fs::write(e, b"VSH1 broken").unwrap();
=======
        let runs = AtomicU64::new(0);
        let translate = |_: &[u8]| {
            runs.fetch_add(1, Ordering::Relaxed);
            Ok(shader())
        };
        assert_eq!(cached(&cache, &build, container, translate, false), Ok(shader()));
        assert_eq!(cached(&cache, &build, container, translate, false), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 1, "the second came from disk");

        // Another build translates again.
        assert_eq!(cached(&cache, &[8; 32], container, translate, false), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 2);

        // A damaged entry is a miss, translated and stored again.
        for e in walk(dir.path()) {
            std::fs::write(e, b"VSH2 broken").unwrap();
>>>>>>> avatar/main
        }
<<<<<<< HEAD
        assert_eq!(
            cached(&cache, &[8; 32], target, container, translate, || true),
            (Ok(shader()), CacheStatus::Stored)
        );
||||||| 85d564167
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);
        assert_eq!(cached(&cache, &build, container, translate), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);
=======
        assert_eq!(cached(&cache, &build, container, translate, false), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);
        assert_eq!(cached(&cache, &build, container, translate, false), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 3);
>>>>>>> avatar/main

<<<<<<< HEAD
        let path = cache.path_for(&shader_key(&build, target, container));
        let mut damaged = std::fs::read(&path).unwrap();
        damaged[40] ^= 1;
        std::fs::write(&path, damaged).unwrap();
        assert_eq!(entry(), (Ok(shader()), CacheStatus::Stored));
        assert_eq!(entry(), (Ok(shader()), CacheStatus::Hit));
||||||| 85d564167
        // Failures are not stored.
        let fail = |_: &[u8]| Err::<HostShader, _>("unsupported".to_owned());
        assert!(cached(&cache, &build, b"other", fail).is_err());
=======
        // Failures are not stored.
        let fail = |_: &[u8]| Err::<HostShader, _>("unsupported".to_owned());
        assert!(cached(&cache, &build, b"other", fail, false).is_err());
>>>>>>> avatar/main
    }

    #[test]
    fn keeps_unsupported_results_but_not_invalid_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let build = [7u8; 32];
        let target = [32, 999_999];
        let error = || ShaderError::Unsupported {
            offset: 4,
            what: "test opcode",
        };
        let first = cached(
            &cache,
            &build,
            target,
            b"unsupported",
            |_| Err(error()),
            || true,
        );
        assert_eq!(first, (Err(error().to_string()), CacheStatus::Stored));
        let second = cached(
            &cache,
            &build,
            target,
            b"unsupported",
            |_| panic!("cache miss"),
            || true,
        );
        assert_eq!(second, (Err(error().to_string()), CacheStatus::Hit));
        let invalid = cached(
            &cache,
            &build,
            target,
            b"invalid",
            |_| Err(ShaderError::NotDxbc),
            || true,
        );
        assert_eq!(invalid.1, CacheStatus::Unstored);
        assert!(
            !cache
                .path_for(&shader_key(&build, target, b"invalid"))
                .exists()
        );
    }

    #[test]
    fn cache_write_failure_keeps_the_translation() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let build = [7u8; 32];
        let target = [32, 999_999];
        let container = b"shader";
        std::fs::create_dir_all(cache.path_for(&shader_key(&build, target, container))).unwrap();
        let (translated, status) =
            cached(&cache, &build, target, container, |_| Ok(shader()), || true);
        assert_eq!(translated, Ok(shader()));
        assert!(matches!(status, CacheStatus::Failed(_)));
    }

    #[test]
    fn canonical_container_ignores_padding_only_for_valid_header_lengths() {
        let mut bytes = [0; 40];
        bytes[..4].copy_from_slice(b"DXBC");
        bytes[0x18..0x1c].copy_from_slice(&32u32.to_le_bytes());
        assert_eq!(canonical_container(&bytes), &bytes[..32]);
        bytes[0x18..0x1c].copy_from_slice(&41u32.to_le_bytes());
        assert_eq!(canonical_container(&bytes), &bytes);
    }

    #[test]
    fn entries_round_trip_and_reject_damage() {
<<<<<<< HEAD
        let bytes = encode(&shader());
        assert_eq!(decode(&bytes), Some(shader()));
        let mut read_only = shader();
        read_only.writes_guest_memory = false;
        assert_eq!(decode(&encode(&read_only)), Some(read_only));
        let mut stale = bytes.clone();
        stale[..4].copy_from_slice(b"VSH1");
        assert_eq!(decode(&stale), None);
||||||| 85d564167
        let bytes = encode(&shader());
        assert_eq!(decode(&bytes), Some(shader()));
=======
        let bytes = encode(&shader(), "made by");
        assert_eq!(decode(&bytes), Some((shader(), b"made by".to_vec())));
>>>>>>> avatar/main
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode(&[bytes.as_slice(), &[0]].concat()), None);
        assert_eq!(decode(b"VSH0"), None);
        // Entries of the encoding before the translator's build was in them.
        let mut v1 = bytes.clone();
        v1[3] = b'1';
        assert_eq!(decode(&v1), None);
    }

    /// An entry an earlier build of the translator made, under the same
    /// version: served as it is; when refreshing translated again in its
    /// place, and a different result counted.
    #[test]
    fn entries_of_an_earlier_translator_are_served_or_refreshed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let build = [9u8; 32];
        let container = b"a container of an earlier build".as_slice();
        let runs = AtomicU64::new(0);
        let old = || {
            let mut s = shader();
            s.dxil = b"the old translation".to_vec();
            s
        };
        let first = |_: &[u8]| {
            runs.fetch_add(1, Ordering::Relaxed);
            Ok(old())
        };
        assert_eq!(cached_by(&cache, &build, "build a", container, first, false), Ok(old()));
        let now = |_: &[u8]| {
            runs.fetch_add(1, Ordering::Relaxed);
            Ok(shader())
        };
        let (stale, refreshed, changed) = stale_counts();
        // The next build, same version: the old translation, not translated.
        assert_eq!(cached_by(&cache, &build, "build b", container, now, false), Ok(old()));
        assert_eq!(runs.load(Ordering::Relaxed), 1);
        // Refreshing: translated again and kept.
        assert_eq!(cached_by(&cache, &build, "build b", container, now, true), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 2);
        // Now the entry is this build's: served when refreshing too.
        assert_eq!(cached_by(&cache, &build, "build b", container, now, true), Ok(shader()));
        assert_eq!(runs.load(Ordering::Relaxed), 2);
        // (Other tests count too; these at least.)
        let (s, r, c) = stale_counts();
        assert!(s >= stale + 2 && r > refreshed && c > changed, "{s} {r} {c}");
    }
}
