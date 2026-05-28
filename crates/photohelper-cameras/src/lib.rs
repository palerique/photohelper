//! Camera profile registry for photohelper.
//!
//! See `docs/plans/session-01.md` §Deliverables 3.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::sync::Arc;

use photohelper_core::Error;
use photohelper_core::model::{CameraId, KnownCamera};

/// Camera profile contract. Most methods are stubs in v0.1 — session 02
/// fills them with real per-ISO noise models, color matrices, and sensor
/// layouts. Stubs return typed `Err(Error::CameraProfileNotImplemented)`
/// — never `panic!` / `todo!()` / `unimplemented!()`.
pub trait CameraProfile: Send + Sync {
    /// Stable identity for this camera.
    fn id(&self) -> CameraId;

    /// Manufacturer + body name as EXIF reports them (Make, Model).
    fn make_model(&self) -> (&'static str, &'static str);

    /// Native base ISO. Stub in v0.1.
    ///
    /// # Errors
    /// Returns `Error::CameraProfileNotImplemented` until session 02.
    fn base_iso(&self) -> Result<u32, Error> {
        Err(Error::CameraProfileNotImplemented {
            method: "base_iso",
            camera_id: self.id(),
        })
    }

    /// Sensor layout (Bayer, X-Trans, …). Stub in v0.1.
    ///
    /// # Errors
    /// Returns `Error::CameraProfileNotImplemented` until session 02.
    fn sensor_layout(&self) -> Result<SensorLayout, Error> {
        Err(Error::CameraProfileNotImplemented {
            method: "sensor_layout",
            camera_id: self.id(),
        })
    }

    /// Camera-to-XYZ color matrix at D65. Stub in v0.1.
    ///
    /// # Errors
    /// Returns `Error::CameraProfileNotImplemented` until session 02.
    fn color_matrix_d65(&self) -> Result<ColorMatrix3x3, Error> {
        Err(Error::CameraProfileNotImplemented {
            method: "color_matrix_d65",
            camera_id: self.id(),
        })
    }

    /// Per-ISO noise model. Stub in v0.1.
    ///
    /// # Errors
    /// Returns `Error::CameraProfileNotImplemented` until session 02.
    fn noise_model(&self, _iso: u32) -> Result<NoiseModel, Error> {
        Err(Error::CameraProfileNotImplemented {
            method: "noise_model",
            camera_id: self.id(),
        })
    }
}

/// Sensor color-filter-array layout. Stub for v0.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SensorLayout {
    /// Bayer RGGB pattern.
    BayerRggb,
}

/// 3x3 color-conversion matrix. Stub for v0.1.
#[derive(Clone, Copy, Debug)]
pub struct ColorMatrix3x3(pub [[f32; 3]; 3]);

/// Per-ISO noise model. Stub for v0.1.
#[derive(Clone, Copy, Debug)]
pub struct NoiseModel {
    /// Read noise (electrons).
    pub read_noise: f32,
    /// Shot noise multiplier.
    pub shot_noise: f32,
}

// =====================================================================
// CanonR8 — the one v0.1 profile
// =====================================================================

/// Canon EOS R8 stub profile (EXIF identification only in v0.1).
#[derive(Debug, Default)]
pub struct CanonR8;

impl CameraProfile for CanonR8 {
    fn id(&self) -> CameraId {
        CameraId::Known(KnownCamera::CanonR8)
    }
    fn make_model(&self) -> (&'static str, &'static str) {
        ("Canon", "Canon EOS R8")
    }
}

// =====================================================================
// CameraRegistry
// =====================================================================

/// Registry of recognized camera bodies. Looks up by EXIF make + model.
pub struct CameraRegistry {
    profiles: Vec<Arc<dyn CameraProfile>>,
}

impl Default for CameraRegistry {
    fn default() -> Self {
        Self {
            profiles: vec![Arc::new(CanonR8)],
        }
    }
}

impl CameraRegistry {
    /// Look up by EXIF make + model. Strips trailing NUL bytes and
    /// surrounding whitespace; case-sensitive on the rest.
    pub fn for_exif(&self, make: &str, model: &str) -> Option<Arc<dyn CameraProfile>> {
        let make = normalize(make);
        let model = normalize(model);
        for p in &self.profiles {
            let (m, mo) = p.make_model();
            if m == make && mo == model {
                return Some(Arc::clone(p));
            }
        }
        None
    }
}

fn normalize(s: &str) -> &str {
    s.trim_end_matches('\0').trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_exif_canon_r8_matches() {
        let r = CameraRegistry::default();
        let p = r
            .for_exif("Canon", "Canon EOS R8")
            .expect("Canon R8 must be registered");
        assert_eq!(p.id(), CameraId::Known(KnownCamera::CanonR8));
    }

    #[test]
    fn for_exif_unknown_returns_none_not_panic() {
        let r = CameraRegistry::default();
        assert!(r.for_exif("Acme", "X1").is_none());
    }

    #[test]
    fn for_exif_strips_trailing_nul_and_whitespace() {
        let r = CameraRegistry::default();
        assert!(r.for_exif("Canon\0", "Canon EOS R8  ").is_some());
        assert!(r.for_exif("  Canon  ", "  Canon EOS R8\0\0").is_some());
    }

    #[test]
    fn for_exif_is_case_sensitive_on_model() {
        let r = CameraRegistry::default();
        // Lower-case 'canon' does NOT match — Canon's EXIF is stable;
        // we don't paper over inputs.
        assert!(r.for_exif("canon", "canon eos r8").is_none());
    }

    #[test]
    fn camera_profile_stub_method_returns_typed_error_not_panic() {
        let r = CanonR8;
        let err = r.base_iso().unwrap_err();
        assert!(matches!(
            err,
            Error::CameraProfileNotImplemented {
                method: "base_iso",
                ..
            }
        ));
    }

    #[test]
    fn camera_profile_all_stubs_return_named_method() {
        let r = CanonR8;
        for (name, result) in [
            ("base_iso", r.base_iso().map(|_| ())),
            ("sensor_layout", r.sensor_layout().map(|_| ())),
            ("color_matrix_d65", r.color_matrix_d65().map(|_| ())),
            ("noise_model", r.noise_model(100).map(|_| ())),
        ] {
            let err = result.unwrap_err();
            match err {
                Error::CameraProfileNotImplemented { method, .. } => {
                    assert_eq!(method, name);
                }
                other => panic!("expected NotImplemented for {name}, got {other:?}"),
            }
        }
    }
}
