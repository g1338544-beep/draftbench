//! A Project Curve link onto a surface (`on_surface`) can name a model edge (`edge_body`, `edge_at`)
//! instead of a sketch curve, so the edge is projected in its true 3D shape. Older designs have
//! neither field and need no rewriting; the last release refuses a design that has them (it reads
//! the unknown fields as part of a sketch-curve link that no longer resolves).
