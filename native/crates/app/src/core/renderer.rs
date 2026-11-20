//! The renderer: the drawing crate (`mcbench_draw::live::Draw`, [`draw`]),
//! started once by the live world when setup has the assets, and given to
//! the tools as the process's one renderer
//! (`mcbench_tools::shots::SharedRenderer`, `LiveParts::shared`): the live
//! view and `see_build` draw with the same `Draw`. Shared with `headless
//! turn` and `headless shots` (`mcbench_headless::renderer`), re-exported.

pub use mcbench_headless::renderer::*;
