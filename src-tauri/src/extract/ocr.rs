//! OCR with Apple Vision (`VNRecognizeTextRequest` revision 3, accurate, language correction).
use std::path::Path;

use objc2::rc::{autoreleasepool, Retained};
use objc2::AnyThread;
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizeTextRequestRevision3, VNRequest,
    VNRequestTextRecognitionLevel,
};

/// Recognition languages, in preference order; only those Vision supports are requested.
const LANGUAGES: [&str; 3] = ["vi-VT", "en-US", "zh-Hans"];

/// Recognised lines (top candidate each) joined with `\n`. Blocking; call off the async runtime.
pub fn recognize(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("non-UTF-8 path")?;
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        // SAFETY: an empty options dictionary is valid for `initWithURL:options:`.
        let handler = unsafe {
            VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new())
        };
        let request = VNRecognizeTextRequest::new();
        // SAFETY: revision 3 is a supported `VNRecognizeTextRequest` revision (macOS 13+).
        unsafe { request.setRevision(VNRecognizeTextRequestRevision3) };
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        // SAFETY: plain getter; returns an error object instead of throwing.
        let supported = unsafe { request.supportedRecognitionLanguagesAndReturnError() }
            .map_err(|e| e.localizedDescription().to_string())?;
        let langs: Vec<Retained<NSString>> = LANGUAGES
            .iter()
            .map(|l| NSString::from_str(l))
            .filter(|l| supported.containsObject(l))
            .collect();
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&langs));

        let base: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[base]))
            .map_err(|e| e.localizedDescription().to_string())?;
        let lines: Vec<String> = request
            .results()
            .map(|obs| {
                obs.iter()
                    .filter_map(|o| o.topCandidates(1).firstObject())
                    .map(|t| t.string().to_string())
                    .collect()
            })
            .unwrap_or_default();
        Ok(lines.join("\n"))
    })
}
