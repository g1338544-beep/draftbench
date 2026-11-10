//! A Project Curve link (a sketch curve projected onto a body) can be saved with `"all_faces": true`:
//! the curve lands on every face of the body, not only the face through its `at` point. Older
//! designs have none and need no rewriting; the last release refuses a design that has one (it reads
//! the newer format number).
