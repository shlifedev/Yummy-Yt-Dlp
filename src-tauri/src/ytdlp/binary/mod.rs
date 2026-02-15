mod dep_check;
pub(crate) mod path;
pub(crate) mod resolve;

// Re-export public API to preserve existing import paths
pub use dep_check::{
    check_full_dependencies, get_cached_dep_status, invalidate_dep_cache, warmup_ytdlp,
};
pub use path::command_with_path_app;
pub use resolve::{
    check_dependencies, resolve_ffmpeg_path_with_app, resolve_ytdlp_path_with_app, update_ytdlp,
};
