//! Standalone Image Signal Processor (ISP) for converting 16-bit linear RAW to 8-bit sRGB.

/// Lookup Table (LUT) for O(1) tone mapping of 16-bit linear samples to 8-bit sRGB.
pub struct ToneMappingLut {
    lut: Box<[u8; 65536]>,
}

impl ToneMappingLut {
    /// Create a new LUT with the given exposure compensation.
    /// Incorporates exposure, an ACES-like S-Curve, and the sRGB OETF.
    pub fn new(exposure_ev: f32) -> Self {
        let mut lut = Box::new([0u8; 65536]);
        let multiplier = 2.0f32.powf(exposure_ev);

        // ACES filmic curve fit parameters
        let a = 2.51;
        let b = 0.03;
        let c = 2.43;
        let d = 0.59;
        let e = 0.14;

        for (i, val) in lut.iter_mut().enumerate() {
            // 1. Convert to linear float [0.0, 1.0]
            let linear = i as f32 / 65535.0;

            // 2. Apply exposure
            let exposed = linear * multiplier;

            // 3. Apply ACES S-Curve
            let mapped = (exposed * (a * exposed + b)) / (exposed * (c * exposed + d) + e);
            let clamped = mapped.clamp(0.0, 1.0);

            // 4. Apply sRGB OETF
            let srgb = if clamped <= 0.0031308 {
                12.92 * clamped
            } else {
                1.055 * clamped.powf(1.0 / 2.4) - 0.055
            };

            *val = (srgb * 255.0).round().clamp(0.0, 255.0) as u8;
        }

        Self { lut }
    }

    /// Map a 16-bit linear sample to an 8-bit sRGB sample.
    #[inline(always)]
    pub fn apply(&self, sample: u16) -> u8 {
        self.lut[sample as usize]
    }
}
