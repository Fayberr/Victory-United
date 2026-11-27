//! Streamline camera constants derived from a FidelityFX dispatch.

use super::abi;

// FidelityFX API upscale flags (`FFX_UPSCALE_ENABLE_*`).
pub const FFX_HIGH_DYNAMIC_RANGE: u32 = 1 << 0;
pub const FFX_DISPLAY_RESOLUTION_MOTION_VECTORS: u32 = 1 << 1;
pub const FFX_MOTION_VECTORS_JITTER_CANCELLATION: u32 = 1 << 2;
pub const FFX_DEPTH_INVERTED: u32 = 1 << 3;

/// What an upscaling says of the camera, and of how to read its inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub jitter: [f32; 2],
    /// Multiplies the motion vectors into render pixels (FidelityFX).
    pub motion_scale: [f32; 2],
    pub render: [u32; 2],
    pub output: [u32; 2],
    pub near: f32,
    pub far: f32,
    pub fov_y: f32,
    pub reset: bool,
    /// The context's creation flags (`FFX_*` above).
    pub flags: u32,
}

impl Camera {
    pub fn depth_inverted(&self) -> bool {
        self.flags & FFX_DEPTH_INVERTED != 0
    }

    pub fn display_motion_vectors(&self) -> bool {
        self.flags & FFX_DISPLAY_RESOLUTION_MOTION_VECTORS != 0
    }

    /// The camera with its nearer plane as the near one. FidelityFX takes
    /// the two distances in either order (its upscaler reads the smaller as
    /// the near plane and lets the inverted-depth flag say which end of the
    /// depth range it is), and the title, whose depth is inverted, names the
    /// far plane first (near 10000, far 0.15 in the first dispatch of
    /// 2026-11-26). Streamline's constants name them as they are.
    pub fn planes_in_order(&self) -> Camera {
        let mut camera = *self;
        if self.near > self.far && self.far > 0.0 {
            (camera.near, camera.far) = (self.far, self.near);
        }
        camera
    }

    /// The size the motion vectors cover.
    pub fn motion_size(&self) -> [u32; 2] {
        if self.display_motion_vectors() {
            self.output
        } else {
            self.render
        }
    }

    /// Streamline's motion vector scale: it multiplies by the render size
    /// itself before handing the scale to DLSS, which then gets FidelityFX's
    /// scale back.
    pub fn mvec_scale(&self, multiplier: [f32; 2]) -> [f32; 2] {
        let size = self.motion_size();
        [0, 1].map(|i| self.motion_scale[i] * multiplier[i] / size[i].max(1) as f32)
    }

    /// The common constants of a frame. The title names no camera matrices:
    /// the projection is made of the field of view, the aspect and the depth
    /// range it gives; camera motion is in the motion vectors, so the
    /// clip-to-previous-clip matrices stay the identity and the camera stands
    /// at the origin looking down +z.
    pub fn constants(
        &self,
        jitter_multiplier: [f32; 2],
        mvec_multiplier: [f32; 2],
    ) -> abi::Constants {
        let aspect = self.render[0].max(1) as f32 / self.render[1].max(1) as f32;
        let view_to_clip = projection(
            self.near,
            self.far,
            self.fov_y,
            aspect,
            self.depth_inverted(),
        );
        let clip_to_view = invert(&view_to_clip).unwrap_or(IDENTITY);
        abi::Constants {
            base: abi::BaseStructure::new(abi::CONSTANTS, 2),
            camera_view_to_clip: view_to_clip,
            clip_to_camera_view: clip_to_view,
            clip_to_lens_clip: IDENTITY,
            clip_to_prev_clip: IDENTITY,
            prev_clip_to_clip: IDENTITY,
            jitter_offset: [
                self.jitter[0] * jitter_multiplier[0],
                self.jitter[1] * jitter_multiplier[1],
            ],
            mvec_scale: self.mvec_scale(mvec_multiplier),
            camera_pinhole_offset: [0.0, 0.0],
            camera_pos: [0.0, 0.0, 0.0],
            camera_up: [0.0, 1.0, 0.0],
            camera_right: [1.0, 0.0, 0.0],
            camera_fwd: [0.0, 0.0, 1.0],
            camera_near: self.near,
            camera_far: self.far,
            camera_fov: self.fov_y,
            camera_aspect_ratio: aspect,
            motion_vectors_invalid_value: 0.0,
            depth_inverted: abi::boolean(self.depth_inverted()),
            camera_motion_included: abi::TRUE,
            motion_vectors_3d: abi::FALSE,
            reset: abi::boolean(self.reset),
            orthographic_projection: abi::FALSE,
            motion_vectors_dilated: abi::FALSE,
            motion_vectors_jittered: abi::boolean(
                self.flags & FFX_MOTION_VECTORS_JITTER_CANCELLATION != 0,
            ),
            min_relative_linear_depth_object_separation: 40.0,
        }
    }
}

pub const IDENTITY: abi::Float4x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A left-handed perspective projection in Streamline's layout (row vectors,
/// rows as stored): depth 0 at `near` and 1 at `far`, or reversed; a far
/// plane past 1e30 (or not finite) is at infinity. Degenerate inputs give
/// the identity.
pub fn projection(near: f32, far: f32, fov_y: f32, aspect: f32, inverted: bool) -> abi::Float4x4 {
    if !(near > 0.0 && fov_y > 0.0 && fov_y < std::f32::consts::PI && aspect > 0.0) || near.is_nan()
    {
        return IDENTITY;
    }
    let y = 1.0 / (fov_y * 0.5).tan();
    let x = y / aspect;
    let infinite = !far.is_finite() || far > 1e30 || far <= near;
    let (a, b) = match (infinite, inverted) {
        (false, false) => (far / (far - near), -near * far / (far - near)),
        (false, true) => (near / (near - far), far * near / (far - near)),
        (true, false) => (1.0, -near),
        (true, true) => (0.0, near),
    };
    [
        [x, 0.0, 0.0, 0.0],
        [0.0, y, 0.0, 0.0],
        [0.0, 0.0, a, 1.0],
        [0.0, 0.0, b, 0.0],
    ]
}

/// A 4x4 inverse by cofactors; `None` if singular.
pub fn invert(m: &abi::Float4x4) -> Option<abi::Float4x4> {
    let a: [f64; 16] = std::array::from_fn(|i| f64::from(m[i / 4][i % 4]));
    let mut inv = [0f64; 16];
    inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15]
        + a[9] * a[7] * a[14]
        + a[13] * a[6] * a[11]
        - a[13] * a[7] * a[10];
    inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15]
        - a[8] * a[7] * a[14]
        - a[12] * a[6] * a[11]
        + a[12] * a[7] * a[10];
    inv[8] = a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15]
        + a[8] * a[7] * a[13]
        + a[12] * a[5] * a[11]
        - a[12] * a[7] * a[9];
    inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14]
        - a[8] * a[6] * a[13]
        - a[12] * a[5] * a[10]
        + a[12] * a[6] * a[9];
    inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15]
        - a[9] * a[3] * a[14]
        - a[13] * a[2] * a[11]
        + a[13] * a[3] * a[10];
    inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15]
        + a[8] * a[3] * a[14]
        + a[12] * a[2] * a[11]
        - a[12] * a[3] * a[10];
    inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15]
        - a[8] * a[3] * a[13]
        - a[12] * a[1] * a[11]
        + a[12] * a[3] * a[9];
    inv[13] = a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14]
        + a[8] * a[2] * a[13]
        + a[12] * a[1] * a[10]
        - a[12] * a[2] * a[9];
    inv[2] = a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15]
        + a[5] * a[3] * a[14]
        + a[13] * a[2] * a[7]
        - a[13] * a[3] * a[6];
    inv[6] = -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15]
        - a[4] * a[3] * a[14]
        - a[12] * a[2] * a[7]
        + a[12] * a[3] * a[6];
    inv[10] = a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15]
        + a[4] * a[3] * a[13]
        + a[12] * a[1] * a[7]
        - a[12] * a[3] * a[5];
    inv[14] = -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14]
        - a[4] * a[2] * a[13]
        - a[12] * a[1] * a[6]
        + a[12] * a[2] * a[5];
    inv[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11]
        - a[5] * a[3] * a[10]
        - a[9] * a[2] * a[7]
        + a[9] * a[3] * a[6];
    inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11]
        + a[4] * a[3] * a[10]
        + a[8] * a[2] * a[7]
        - a[8] * a[3] * a[6];
    inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11]
        - a[4] * a[3] * a[9]
        - a[8] * a[1] * a[7]
        + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10]
        + a[4] * a[2] * a[9]
        + a[8] * a[1] * a[6]
        - a[8] * a[2] * a[5];
    let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if det.abs() < 1e-30 || !det.is_finite() {
        return None;
    }
    Some(std::array::from_fn(|r| {
        std::array::from_fn(|c| (inv[r * 4 + c] / det) as f32)
    }))
}

/// `"x,y"` as two multipliers (`VICTORY_DLSS_JITTER`, `VICTORY_DLSS_MV`):
/// what a run may have to flip; unset or malformed is `[1, 1]`.
pub fn pair(value: Option<&str>) -> [f32; 2] {
    let Some(value) = value else {
        return [1.0, 1.0];
    };
    let mut parts = value.split(',').map(|p| p.trim().parse::<f32>());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(x)), Some(Ok(y)), None) if x.is_finite() && y.is_finite() => [x, y],
        _ => [1.0, 1.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(flags: u32) -> Camera {
        Camera {
            jitter: [0.25, -0.5],
            motion_scale: [1706.0, 960.0],
            render: [1706, 960],
            output: [2560, 1440],
            near: 0.1,
            far: 10_000.0,
            fov_y: 1.0,
            reset: true,
            flags,
        }
    }

    #[test]
    fn constants_follow_the_upscaling() {
        let c =
            camera(FFX_HIGH_DYNAMIC_RANGE | FFX_DEPTH_INVERTED).constants([1.0, 1.0], [1.0, 1.0]);
        // Motion vectors in UV space scaled to pixels: Streamline gets them
        // back to [-1, 1].
        assert_eq!(c.mvec_scale, [1.0, 1.0]);
        assert_eq!(c.jitter_offset, [0.25, -0.5]);
        assert_eq!(
            (c.depth_inverted, c.reset, c.camera_motion_included),
            (abi::TRUE, abi::TRUE, abi::TRUE)
        );
        assert_eq!(c.motion_vectors_jittered, abi::FALSE);
        assert!((c.camera_aspect_ratio - 1706.0 / 960.0).abs() < 1e-6);
        let flipped = camera(0).constants([-1.0, 1.0], [1.0, -2.0]);
        assert_eq!(flipped.jitter_offset, [-0.25, -0.5]);
        assert_eq!(flipped.mvec_scale, [1.0, -2.0]);
        assert_eq!(flipped.depth_inverted, abi::FALSE);
        // Display-resolution motion vectors are scaled by the output size.
        let display = camera(FFX_DISPLAY_RESOLUTION_MOTION_VECTORS);
        assert_eq!(display.motion_size(), [2560, 1440]);
        assert_eq!(
            display.mvec_scale([1.0, 1.0]),
            [1706.0 / 2560.0, 960.0 / 1440.0]
        );
    }

    /// Points at the near and far planes land at the depth the convention
    /// says; the inverse maps clip space back.
    #[test]
    fn projections_place_near_and_far() {
        let depth = |m: &abi::Float4x4, z: f32| {
            let (zc, wc) = (z * m[2][2] + m[3][2], z * m[2][3] + m[3][3]);
            zc / wc
        };
        let standard = projection(0.5, 100.0, 1.2, 16.0 / 9.0, false);
        assert!((depth(&standard, 0.5) - 0.0).abs() < 1e-5);
        assert!((depth(&standard, 100.0) - 1.0).abs() < 1e-5);
        let reversed = projection(0.5, 100.0, 1.2, 16.0 / 9.0, true);
        assert!((depth(&reversed, 0.5) - 1.0).abs() < 1e-5);
        assert!((depth(&reversed, 100.0) - 0.0).abs() < 1e-5);
        let infinite = projection(0.5, f32::MAX, 1.2, 1.0, true);
        assert!((depth(&infinite, 0.5) - 1.0).abs() < 1e-5);
        assert!(depth(&infinite, 1e7) < 1e-6);
        assert_eq!(projection(0.0, 100.0, 1.2, 1.0, false), IDENTITY);
        let inverse = invert(&standard).unwrap();
        let product: abi::Float4x4 = std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..4).map(|k| standard[r][k] * inverse[k][c]).sum())
        });
        for r in 0..4 {
            for c in 0..4 {
                let want = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (product[r][c] - want).abs() < 1e-4,
                    "{r} {c} {}",
                    product[r][c]
                );
            }
        }
        assert_eq!(invert(&[[0.0; 4]; 4]), None);
    }

    /// The title names the far plane first (inverted depth): in order, the
    /// projection puts the nearer plane at depth 1 and the farther at 0; as
    /// named, it took the farther for the near plane of an endless range.
    #[test]
    fn planes_named_far_first_are_put_in_order() {
        let mut named = camera(FFX_DEPTH_INVERTED);
        (named.near, named.far) = (10_000.0, 0.15);
        let ordered = named.planes_in_order();
        assert_eq!((ordered.near, ordered.far), (0.15, 10_000.0));
        assert_eq!(ordered.planes_in_order(), ordered);
        let c = ordered.constants([1.0, 1.0], [1.0, 1.0]);
        assert_eq!((c.camera_near, c.camera_far), (0.15, 10_000.0));
        let depth = |z: f32| {
            let m = &c.camera_view_to_clip;
            (z * m[2][2] + m[3][2]) / (z * m[2][3] + m[3][3])
        };
        assert!((depth(0.15) - 1.0).abs() < 1e-5 && depth(10_000.0).abs() < 1e-5);
        // As named: an endless range from 10000 on.
        let wrong = named.constants([1.0, 1.0], [1.0, 1.0]);
        assert_eq!(wrong.camera_view_to_clip[3][2], 10_000.0);
        // An endless far plane, or a plane at 0, is left alone.
        let mut endless = camera(FFX_DEPTH_INVERTED);
        (endless.near, endless.far) = (0.15, f32::MAX);
        assert_eq!(endless.planes_in_order(), endless);
        (endless.near, endless.far) = (0.15, 0.0);
        assert_eq!(endless.planes_in_order(), endless);
    }

    #[test]
    fn multiplier_pairs() {
        assert_eq!(pair(None), [1.0, 1.0]);
        assert_eq!(pair(Some("-1, 1")), [-1.0, 1.0]);
        assert_eq!(pair(Some("1")), [1.0, 1.0]);
        assert_eq!(pair(Some("1,2,3")), [1.0, 1.0]);
        assert_eq!(pair(Some("x,1")), [1.0, 1.0]);
    }
}
