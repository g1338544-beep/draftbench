//! A joint can be saved as `"pinpath"` (Pin in Path: a pin that turns and runs along a slot of
//! any shape), with its slot's centre line as `"path"` (points in the joint frame) and
//! `"path_on_b"` when the slot is on the moving part. Older designs have none and need no
//! rewriting; the last release refuses a design that has one (it reads the newer format number).
