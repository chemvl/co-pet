//! Speaker diarization: sherpa-onnx offline diarization (pyannote segmentation 3.0 + 3D-Speaker
//! embeddings).
use std::collections::HashMap;
use std::path::Path;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::error::{AppError, AppResult};

/// Cosine-distance cut for merging speaker clusters; larger means fewer speakers. 0.5 split one
/// far-field voice into several speakers on a real council meeting; 0.85 kept it whole.
const CLUSTER_THRESHOLD: f32 = 0.85;
/// A cluster with less speech than this (or than 1/20 of all speech, whichever is smaller) is a
/// mis-clustered fragment and is folded into its nearest neighbour in time.
const MINOR_SPEAKER_MS: i64 = 10_000;

/// One speaker's turn on the track's timeline. Speakers are numbered from 0 per run.
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: i32,
}

pub trait Diarizer {
    /// Turns sorted by start time.
    fn diarize(&self, samples_16k: &[f32]) -> AppResult<Vec<Turn>>;
}

pub struct SherpaDiarizer(OfflineSpeakerDiarization);

impl SherpaDiarizer {
    /// Missing or unloadable models are `speakerModel`.
    pub fn load(segmentation: &Path, embedding: &Path) -> AppResult<Self> {
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(segmentation.to_string_lossy().into_owned()),
                    ..Default::default()
                },
                num_threads: 4,
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(embedding.to_string_lossy().into_owned()),
                num_threads: 4,
                ..Default::default()
            },
            clustering: FastClusteringConfig { num_clusters: -1, threshold: CLUSTER_THRESHOLD, ..Default::default() },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };
        OfflineSpeakerDiarization::create(&config).map(SherpaDiarizer).ok_or(AppError::Code("speakerModel"))
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize(&self, samples_16k: &[f32]) -> AppResult<Vec<Turn>> {
        if samples_16k.is_empty() {
            return Ok(Vec::new());
        }
        let result = self.0.process(samples_16k).ok_or_else(|| AppError::msg("diarization failed"))?;
        let turns = result
            .sort_by_start_time()
            .into_iter()
            .map(|s| Turn {
                start_ms: (f64::from(s.start) * 1000.0).round() as i64,
                end_ms: (f64::from(s.end) * 1000.0).round() as i64,
                speaker: s.speaker,
            })
            .collect();
        Ok(merge_minor_speakers(turns))
    }
}

/// Gives every turn of a minor cluster (see [`MINOR_SPEAKER_MS`]) the speaker of the nearest
/// turn in time (smallest gap, earlier on ties) that belongs to a kept cluster. Unchanged when no
/// cluster is large enough to keep.
fn merge_minor_speakers(mut turns: Vec<Turn>) -> Vec<Turn> {
    let mut speech: HashMap<i32, i64> = HashMap::new();
    for t in &turns {
        *speech.entry(t.speaker).or_default() += t.end_ms - t.start_ms;
    }
    let floor = MINOR_SPEAKER_MS.min(speech.values().sum::<i64>() / 20);
    let minor = |speaker: i32| speech[&speaker] < floor;
    let kept: Vec<(i64, i64, i32)> =
        turns.iter().filter(|t| !minor(t.speaker)).map(|t| (t.start_ms, t.end_ms, t.speaker)).collect();
    for t in turns.iter_mut().filter(|t| minor(t.speaker)) {
        let gap = |k: &&(i64, i64, i32)| (k.0 - t.end_ms).max(t.start_ms - k.1).max(0);
        if let Some(k) = kept.iter().min_by_key(gap) {
            t.speaker = k.2;
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models_are_speaker_model_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = SherpaDiarizer::load(&dir.path().join("seg.onnx"), &dir.path().join("emb.onnx")).err().unwrap();
        assert!(matches!(err, AppError::Code("speakerModel")), "{err}");
    }

    fn turns(spec: &[(i64, i64, i32)]) -> Vec<Turn> {
        spec.iter().map(|&(start_ms, end_ms, speaker)| Turn { start_ms, end_ms, speaker }).collect()
    }

    fn speakers(turns: &[Turn]) -> Vec<i32> {
        turns.iter().map(|t| t.speaker).collect()
    }

    #[test]
    fn minor_clusters_join_the_nearest_kept_speaker() {
        // Speakers 0 and 1 talk for minutes; 2 and 3 are sub-10 s fragments of them.
        let merged = merge_minor_speakers(turns(&[
            (0, 60_000, 0),
            (60_500, 63_000, 2),
            (63_500, 120_000, 0),
            (120_000, 180_000, 1),
            (181_000, 182_000, 3),
            (190_000, 250_000, 1),
        ]));
        assert_eq!(speakers(&merged), [0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn short_speakers_survive_in_short_recordings() {
        // 50 s of speech: the floor is 2.5 s, so a 4 s speaker is real and a 1 s one is not.
        let merged = merge_minor_speakers(turns(&[(0, 20_000, 0), (20_000, 24_000, 1), (24_000, 25_000, 2), (25_000, 50_000, 0)]));
        assert_eq!(speakers(&merged), [0, 1, 1, 0]);
    }
}
