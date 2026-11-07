//! A `SurfaceTrim` can be saved with `"remove": true`: its picked points are the regions to take
//! away (everything else stays) instead of the regions to keep. Older designs have none and need no
//! rewriting; the last release refuses a design that has one (it reads the newer format number).
