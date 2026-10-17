//! Speaker diarization: sherpa-onnx offline diarization (pyannote segmentation 3.0 + 3D-Speaker
//! embeddings).
use std::path::Path;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::error::{AppError, AppResult};

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
            clustering: FastClusteringConfig { num_clusters: -1, threshold: 0.5, ..Default::default() },
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
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .map(|s| Turn {
                start_ms: (f64::from(s.start) * 1000.0).round() as i64,
                end_ms: (f64::from(s.end) * 1000.0).round() as i64,
                speaker: s.speaker,
            })
            .collect())
    }
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
}
