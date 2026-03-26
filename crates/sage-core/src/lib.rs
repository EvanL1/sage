pub mod plugin;
pub mod bridge;
pub mod calibrator;
pub mod channels;
pub mod coach;
pub mod config;
pub mod context_gatherer;
pub mod daemon;
pub mod feedback;
pub mod guardian;
pub mod heartbeat;
pub mod memory_evolution;
pub mod memory_integrator;
pub mod mirror;
pub mod oauth2;
pub mod observer;
pub mod onboarding;
pub mod person_observer;
pub mod persona;
pub mod pipeline;
pub mod profile;
pub mod prompts;
pub mod questioner;
pub mod reconciler;
pub mod reflective_detector;
pub mod router;
pub mod session_analyzer;
pub mod staleness;
pub mod skills;
pub mod strategist;
pub mod task_intelligence;

// re-export from sage-store
pub use sage_store as store;
pub use sage_store::similarity;
pub use sage_store::time_normalizer;

// re-export from sage-llm
pub use sage_llm::agent;
pub use sage_llm::discovery;
pub use sage_llm::provider;
pub use sage_llm::AgentConfig;

// re-export from sage-channels
pub use sage_channels::applescript;
pub use sage_channels::channel;

pub use daemon::Daemon;
