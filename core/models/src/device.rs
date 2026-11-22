//! Hardware classification and the model recommendation built on it.
//!
//! Everything here is a pure function of a [`DeviceProfile`] the shell fills
//! from the operating system. Keeping it pure makes the thresholds testable and
//! makes the recommendation reproducible: the same machine always gets the same
//! answer, and the reason shown in the UI is the reason the code used.

use serde::Serialize;

use crate::catalog::{self, ModelPack, PACKS, Tier};

/// What the shell measured. Every field is optional in the sense that an
/// unknown value must be reported as such, never guessed: a wrong RAM figure
/// would recommend a model the machine cannot run.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceProfile {
    pub cpu_model: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    /// Free space on the volume the models are installed to.
    pub free_disk_mb: u64,
    pub os: String,
    pub arch: String,
    pub gpu: Option<String>,
    pub gpu_memory_mb: Option<u64>,
    /// True when this build can execute on the GPU *and* an adapter was found.
    /// The shell decides this from its cargo features and the probe.
    pub gpu_acceleration: bool,
}

/// Transparent hardware class. The score is documented so a user can see why a
/// machine landed where it did; see [`DeviceClass::score`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Basic,
    Standard,
    Advanced,
    HighPerformance,
}

impl DeviceClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Standard => "Standard",
            Self::Advanced => "Advanced",
            Self::HighPerformance => "High performance",
        }
    }

    pub fn label_ar(self) -> &'static str {
        match self {
            Self::Basic => "أساسي",
            Self::Standard => "قياسي",
            Self::Advanced => "متقدم",
            Self::HighPerformance => "عالي الأداء",
        }
    }
}

/// One line of the transparent classification. The UI renders these instead of
/// re-deriving the thresholds, so what the user reads is what [`score`] counted.
#[derive(Clone, Debug, Serialize)]
pub struct Criterion {
    pub key: &'static str,
    pub label: &'static str,
    pub label_ar: &'static str,
    pub met: bool,
    pub points: u8,
    /// The measured value, so a user can check the claim.
    pub detail: String,
}

/// The seven criteria, in the order the advisor shows them.
///
/// | Resource | Points |
/// |---|---|
/// | 4 or more logical cores | 1 |
/// | 8 or more logical cores | 1 |
/// | 16 or more logical cores | 1 |
/// | 8 GB RAM or more | 1 |
/// | 16 GB RAM or more | 1 |
/// | 32 GB RAM or more | 1 |
/// | GPU acceleration available | 1 |
///
/// 0–1 = Basic, 2–3 = Standard, 4–5 = Advanced, 6–7 = High performance.
pub fn criteria(profile: &DeviceProfile) -> Vec<Criterion> {
    let cores = profile.logical_cores;
    let ram = profile.total_ram_mb;
    let ram_gb = ram / 1024;
    let mut list = Vec::with_capacity(7);

    for (key, threshold, label, label_ar) in [
        ("cores4", 4usize, "4+ logical cores", "4+ نواة منطقية"),
        ("cores8", 8, "8+ logical cores", "8+ نواة منطقية"),
        ("cores16", 16, "16+ logical cores", "16+ نواة منطقية"),
    ] {
        let met = cores >= threshold;
        list.push(Criterion {
            key,
            label,
            label_ar,
            met,
            points: u8::from(met),
            detail: format!("{cores}"),
        });
    }
    for (key, threshold, label, label_ar) in [
        ("ram8", 8_192u64, "8 GB RAM or more", "8 غ.ب ذاكرة أو أكثر"),
        ("ram16", 16_384, "16 GB RAM or more", "16 غ.ب ذاكرة أو أكثر"),
        ("ram32", 32_768, "32 GB RAM or more", "32 غ.ب ذاكرة أو أكثر"),
    ] {
        let met = ram >= threshold;
        list.push(Criterion {
            key,
            label,
            label_ar,
            met,
            points: u8::from(met),
            detail: format!("{ram_gb} GB"),
        });
    }

    let gpu = profile.gpu_acceleration;
    list.push(Criterion {
        key: "gpu",
        label: "GPU acceleration",
        label_ar: "تسريع كرت الرسوميات",
        met: gpu,
        points: u8::from(gpu),
        detail: match (&profile.gpu, gpu) {
            (Some(name), true) => name.clone(),
            (Some(name), false) => format!("{name} (CPU build)"),
            (None, _) => "no usable GPU detected".to_string(),
        },
    });
    list
}

pub fn score(profile: &DeviceProfile) -> u8 {
    criteria(profile)
        .iter()
        .map(|criterion| criterion.points)
        .sum()
}

pub fn classify(profile: &DeviceProfile) -> DeviceClass {
    match score(profile) {
        0..=1 => DeviceClass::Basic,
        2..=3 => DeviceClass::Standard,
        4..=5 => DeviceClass::Advanced,
        _ => DeviceClass::HighPerformance,
    }
}

/// The advisor's answer.
#[derive(Clone, Debug, Serialize)]
pub struct Recommendation {
    pub pack_id: &'static str,
    pub device_class: DeviceClass,
    pub score: u8,
    /// One sentence a user can check against the hardware summary.
    pub reason: String,
    pub reason_ar: String,
    /// True when the class's preferred pack was stepped down because of RAM or
    /// disk, so the UI can say why the "obvious" answer was not chosen.
    pub downgraded: bool,
    /// True when even the lightest pack is bigger than the memory budget. The
    /// recommendation is still returned — it is the best available answer — but
    /// the UI must not present it as a comfortable fit.
    pub insufficient_memory: bool,
    /// How many OCR pages the pipeline should process at once on this machine.
    pub ocr_concurrency: usize,
    /// How many files a batch search should scan per worker chunk.
    pub batch_chunk: usize,
    /// Threads for a full index rebuild.
    pub index_threads: usize,
}

/// The pack a class prefers, before RAM and disk checks.
pub fn preferred_pack(class: DeviceClass) -> &'static ModelPack {
    let tier = match class {
        DeviceClass::Basic => Tier::Compact,
        DeviceClass::Standard => Tier::Balanced,
        DeviceClass::Advanced | DeviceClass::HighPerformance => Tier::Precision,
    };
    PACKS
        .iter()
        .find(|pack| pack.tier == tier)
        .unwrap_or_else(|| catalog::default_pack())
}

/// The advisor.
///
/// Rules, in order:
/// 1. Start from the class's preferred tier.
/// 2. Step down while the pack's peak memory plus 25 % does not fit in the
///    memory budget: a quarter of installed RAM, capped by what is free now.
///    A quarter is the comfort line — OCR may not take memory the rest of the
///    machine needs — and the free-RAM cap stops a busy machine from being
///    told to load a model that would swap.
/// 3. Step down while the pack does not fit on disk with headroom.
/// 4. Never recommend below Compact.
pub fn recommend(profile: &DeviceProfile) -> Recommendation {
    let device_class = classify(profile);
    let preferred = preferred_pack(device_class);
    let mut chosen = preferred;
    let mut reason_parts: Vec<String> = Vec::new();
    let mut reason_ar_parts: Vec<String> = Vec::new();
    let mut downgraded = false;

    let ram_budget = ram_budget_mb(profile);
    while chosen.ram_mb.saturating_mul(5) / 4 > ram_budget && chosen.tier > Tier::Compact {
        downgraded = true;
        reason_parts.push(format!(
            "{} needs about {} MB of memory; this machine has {} MB available.",
            chosen.name, chosen.ram_mb, ram_budget
        ));
        reason_ar_parts.push(format!(
            "يحتاج {} نحو {} م.ب من الذاكرة؛ المتاح على هذا الجهاز {} م.ب.",
            chosen.name_ar, chosen.ram_mb, ram_budget
        ));
        chosen = step_down(chosen);
    }

    while !fits_on_disk(chosen, profile.free_disk_mb) && chosen.tier > Tier::Compact {
        downgraded = true;
        reason_parts.push(format!(
            "{} needs {} MB of free disk; {} MB is free.",
            chosen.name,
            chosen.download_bytes() / 1_000_000,
            profile.free_disk_mb
        ));
        reason_ar_parts.push(format!(
            "يحتاج {} نحو {} م.ب مساحة حرة؛ المتاح {} م.ب.",
            chosen.name_ar,
            chosen.download_bytes() / 1_000_000,
            profile.free_disk_mb
        ));
        chosen = step_down(chosen);
    }

    // The loop cannot step below Compact, so a machine that is too small for
    // even the lightest pack left the loop with a pack that does not fit and a
    // reason that only named the packs it had stepped away from. Saying so is the
    // difference between "we picked the smallest" and "nothing here fits".
    let insufficient_memory = chosen.ram_mb.saturating_mul(5) / 4 > ram_budget;
    if insufficient_memory {
        reason_parts.push(format!(
            "Even {} needs about {} MB, more than the {} MB budget on this machine; OCR may be slow or unavailable until memory is freed.",
            chosen.name, chosen.ram_mb, ram_budget
        ));
        reason_ar_parts.push(format!(
            "حتى {} يحتاج نحو {} م.ب، أكثر من الميزانية المتاحة {} م.ب؛ قد تكون قراءة النصوص بطيئة أو غير متاحة حتى تُحرَّر الذاكرة.",
            chosen.name_ar, chosen.ram_mb, ram_budget
        ));
    }

    if reason_parts.is_empty() {
        reason_parts.push(format!(
            "{} matches this {} machine: {} logical cores, {} GB RAM.",
            chosen.name,
            device_class.label().to_lowercase(),
            profile.logical_cores,
            profile.total_ram_mb / 1024
        ));
        reason_ar_parts.push(format!(
            "{} يناسب جهازاً بمستوى {}: {} نواة منطقية و{} غ.ب ذاكرة.",
            chosen.name_ar,
            device_class.label_ar(),
            profile.logical_cores,
            profile.total_ram_mb / 1024
        ));
    }

    let cores = profile.logical_cores.max(1);
    let ocr_concurrency = match chosen.tier {
        Tier::Compact => (cores / 2).clamp(1, 4),
        Tier::Balanced => (cores / 4).clamp(1, 3),
        Tier::Precision => (cores / 4).clamp(1, 2),
    };
    let batch_chunk = (cores * 64).clamp(128, 2_048);
    let index_threads = cores.saturating_sub(1).clamp(1, 8);

    Recommendation {
        pack_id: chosen.id,
        device_class,
        score: score(profile),
        reason: reason_parts.join(" "),
        reason_ar: reason_ar_parts.join(" "),
        downgraded,
        insufficient_memory,
        ocr_concurrency,
        batch_chunk,
        index_threads,
    }
}

/// Memory the advisor may plan with.
///
/// OCR is a background feature, so it may use at most a quarter of installed
/// RAM: the rest belongs to the OS, the index and whatever the user is doing.
/// A busy machine that reports little free memory is capped at that figure, so
/// the advisor never plans an install that would swap. An unmeasured profile is
/// not guessed at: with neither figure the budget is unlimited and the tier
/// alone decides.
///
/// `available_ram_mb == 0` on a machine whose total is known means "nothing is
/// free", not "unknown". Treating it as unknown gave a fully committed 16 GB
/// machine a 4 GB budget and a recommendation of the heaviest pack with
/// `downgraded: false`, which is the one case the free-RAM cap exists to catch.
fn ram_budget_mb(profile: &DeviceProfile) -> u64 {
    match (profile.total_ram_mb, profile.available_ram_mb) {
        (0, 0) => u64::MAX,
        (0, available) => available,
        (total, available) => (total / 4).min(available),
    }
}

fn fits_on_disk(pack: &ModelPack, free_disk_mb: u64) -> bool {
    // An unknown free-space figure must not block an install; the downloader
    // checks again with the real number before it writes.
    if free_disk_mb == 0 {
        return true;
    }
    // One requirement, shared with the downloader, so the advisor cannot refuse
    // a pack the installer would accept or accept one it would refuse.
    free_disk_mb >= crate::install::disk_requirement_mb(pack)
}

fn step_down(pack: &'static ModelPack) -> &'static ModelPack {
    PACKS
        .iter()
        .filter(|candidate| candidate.tier < pack.tier)
        .max_by_key(|candidate| candidate.tier)
        .unwrap_or(pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(cores: usize, ram_mb: u64) -> DeviceProfile {
        DeviceProfile {
            cpu_model: "Test CPU".to_string(),
            physical_cores: cores / 2,
            logical_cores: cores,
            total_ram_mb: ram_mb,
            available_ram_mb: ram_mb / 2,
            free_disk_mb: 100_000,
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            gpu: None,
            gpu_memory_mb: None,
            gpu_acceleration: false,
        }
    }

    #[test]
    fn a_two_core_low_ram_machine_is_basic_and_gets_the_compact_pack() {
        let device = profile(2, 4_096);
        assert_eq!(classify(&device), DeviceClass::Basic);
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-mobile");
        assert!(!answer.downgraded);
    }

    #[test]
    fn a_four_core_eight_gigabyte_machine_is_standard() {
        let device = profile(4, 8_192);
        assert_eq!(classify(&device), DeviceClass::Standard);
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-server-det");
    }

    #[test]
    fn eight_cores_sixteen_gigabytes_is_advanced() {
        let device = profile(8, 16_384);
        assert_eq!(classify(&device), DeviceClass::Advanced);
        assert_eq!(recommend(&device).pack_id, "ppocrv5-server");
    }

    #[test]
    fn a_workstation_is_high_performance() {
        let device = profile(16, 65_536);
        assert_eq!(classify(&device), DeviceClass::HighPerformance);
        assert_eq!(recommend(&device).pack_id, "ppocrv5-server");
    }

    #[test]
    fn gpu_acceleration_adds_a_point() {
        let mut device = profile(4, 8_192);
        let before = score(&device);
        device.gpu_acceleration = true;
        assert_eq!(score(&device), before + 1);
    }

    #[test]
    fn a_small_machine_never_recommends_a_pack_that_would_swap() {
        // 16 logical cores but only 4 GB of RAM: the class says Precision, the
        // memory check must override it.
        let device = profile(16, 4_096);
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-mobile");
        assert!(answer.downgraded);
        assert!(answer.reason.contains("memory"));
    }

    #[test]
    fn a_full_disk_steps_down() {
        let mut device = profile(16, 65_536);
        // Enough for the compact pack (about 155 MB) and not for the balanced
        // one (about 359 MB).
        device.free_disk_mb = 300;
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-mobile");
        assert!(answer.downgraded);
        assert!(answer.reason.contains("disk"));
    }

    /// The advisor and the downloader share one disk requirement. They used to
    /// disagree by a factor of eight — the advisor demanded two gigabytes of
    /// headroom — so a device with 500 MB free was told the Precision pack did
    /// not fit and then allowed to install it.
    #[test]
    fn a_disk_that_fits_the_installer_fits_the_advisor() {
        let mut device = profile(16, 65_536);
        let pack = crate::catalog::pack("ppocrv5-server").expect("precision pack");
        device.free_disk_mb = crate::install::disk_requirement_mb(pack);
        assert_eq!(recommend(&device).pack_id, "ppocrv5-server");
        assert!(!recommend(&device).downgraded);
    }

    /// The step-down loop cannot go below the compact pack, so a machine too
    /// small for even that used to be told the compact pack "matches this
    /// machine" with `downgraded: true` and no word about it not fitting.
    #[test]
    fn a_machine_too_small_for_every_pack_says_so() {
        let device = profile(16, 1_024);
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-mobile");
        assert!(
            answer.insufficient_memory,
            "the lightest pack needs more than a 256 MB budget: {answer:?}"
        );
        assert!(
            answer.reason.contains("Even "),
            "the reason must say the chosen pack does not fit either: {}",
            answer.reason
        );
        assert!(answer.reason_ar.contains("حتى "), "{}", answer.reason_ar);
    }

    /// Zero available memory on a machine whose total is known means the machine
    /// is full, not that the figure is unknown. Reading it as unknown handed a
    /// committed 16 GB machine a 4 GB budget and the heaviest pack.
    #[test]
    fn no_free_memory_is_treated_as_full_not_unknown() {
        let mut device = profile(16, 16_384);
        device.available_ram_mb = 0;
        let answer = recommend(&device);
        assert_eq!(answer.pack_id, "ppocrv4-mobile");
        assert!(answer.insufficient_memory);
        assert!(answer.downgraded);
    }

    /// Neither figure known is still "unmeasured", and an unmeasured machine is
    /// not punished for it.
    #[test]
    fn an_unmeasured_profile_is_not_treated_as_a_full_one() {
        let mut device = profile(16, 65_536);
        device.total_ram_mb = 0;
        device.available_ram_mb = 0;
        let answer = recommend(&device);
        // The class itself drops a point without a RAM figure, so the pack is
        // whatever that class prefers; what matters here is that the memory
        // budget is unlimited rather than zero.
        assert_eq!(answer.pack_id, preferred_pack(answer.device_class).id);
        assert!(!answer.insufficient_memory);
        assert!(!answer.reason.contains("memory"), "{}", answer.reason);
    }

    #[test]
    fn an_unknown_disk_figure_does_not_block_the_best_pack() {
        let mut device = profile(16, 65_536);
        device.free_disk_mb = 0;
        assert_eq!(recommend(&device).pack_id, "ppocrv5-server");
    }

    #[test]
    fn concurrency_is_bounded_and_at_least_one() {
        for cores in [1, 2, 4, 8, 16, 64] {
            for ram in [2_048, 8_192, 32_768] {
                let answer = recommend(&profile(cores, ram));
                assert!((1..=4).contains(&answer.ocr_concurrency), "{cores}/{ram}");
                assert!((1..=8).contains(&answer.index_threads));
                assert!((128..=2_048).contains(&answer.batch_chunk));
            }
        }
    }

    #[test]
    fn the_score_is_the_sum_of_the_published_criteria() {
        let device = profile(8, 16_384);
        let list = criteria(&device);
        assert_eq!(list.len(), 7);
        assert_eq!(
            score(&device),
            list.iter().map(|criterion| criterion.points).sum::<u8>()
        );
        assert_eq!(list.iter().filter(|criterion| criterion.met).count(), 4);
    }

    #[test]
    fn the_reason_names_the_hardware_it_used() {
        let answer = recommend(&profile(8, 16_384));
        assert!(
            answer.reason.contains("8 logical cores"),
            "{}",
            answer.reason
        );
        assert!(answer.reason.contains("16 GB RAM"), "{}", answer.reason);
        assert!(!answer.reason_ar.is_empty());
    }
}
