mod aggregate;
mod bottleneck;
mod context;
mod detectors;
#[cfg(test)]
mod tests;

pub use aggregate::aggregate_suspects;
pub use context::SuspectContext;
pub use detectors::{
    detect_broadcast_join, detect_cache_opportunity, detect_cpu_efficiency, detect_memory_pressure,
    detect_partition_count, detect_python_udf, detect_record_explosion, detect_slow_stages,
    detect_spill, detect_task_failures,
};
