//! A sketch can be saved with `"role": "curve"` (inside its `view`): the result of Project Curve,
//! which the browser lists under Curves instead of Sketches, or `"role": "helper"`: the working
//! sketch such a projection reads from, which the browser does not list. Older designs have
//! none and need no rewriting; the last release refuses a design that has one (it reads the
//! newer format number).
