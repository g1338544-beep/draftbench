//! A Hole feature can be saved as `{"type": "tapered", "taper_angle": …}`: a conical hole whose
//! `diameter` is its width at the face, narrowing inwards by `taper_angle` on each side. Older
//! designs have no such hole and need no rewriting; the last release refuses a design that has
//! one (it reads the newer format number).
