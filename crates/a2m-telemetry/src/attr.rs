//! What a span or a metric may say about itself.
//!
//! Spans and metrics carry names, timings, and outcomes only (spec section
//! 217). The enforcement is the type: an attribute is one of the values
//! below, each a closed vocabulary, a checked name, a digest, or a count.
//! There is no way to attach free text, so there is no way to attach a value
//! a system returned.

use a2m_core::{
    Digest, EffectClass, FailureClass, FailureCode, Mode, OperationName, StepKind, TargetName,
};

use crate::trace::CaptureLevel;

/// One attribute of a span or of a metric's series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attr {
    /// The MCP method of a call.
    Method(McpMethod),
    /// The tool a call names.
    Tool(OperationName),
    /// The operation being carried out.
    Operation(OperationName),
    /// The target the operation belongs to.
    Target(TargetName),
    /// The plan that was run, by its hash: the tool's version.
    Plan(Digest),
    /// The mode of the binding in use.
    Mode(Mode),
    /// The kind of step.
    StepKind(StepKind),
    /// The effect class of the operation.
    Effect(EffectClass),
    /// The capture level of the run.
    Level(CaptureLevel),
    /// The class of the failure that ended the work.
    Class(FailureClass),
    /// The code of that failure.
    Code(FailureCode),
    /// How many of something: records, steps, retries.
    Count(u64),
}

impl Attr {
    /// The attribute's key. Every spelling lives in this one table. The two
    /// MCP keys follow OpenTelemetry's conventions for MCP, which are still
    /// marked as in development and may be renamed.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Method(_) => "mcp.method.name",
            Self::Tool(_) => "gen_ai.tool.name",
            Self::Operation(_) => "a2m.operation",
            Self::Target(_) => "a2m.target",
            Self::Plan(_) => "a2m.plan",
            Self::Mode(_) => "a2m.mode",
            Self::StepKind(_) => "a2m.step.kind",
            Self::Effect(_) => "a2m.effect",
            Self::Level(_) => "a2m.capture.level",
            Self::Class(_) => "a2m.failure.class",
            Self::Code(_) => "a2m.failure.code",
            Self::Count(_) => "a2m.count",
        }
    }

    /// The attribute's value, as text.
    #[must_use]
    pub fn value(&self) -> String {
        match self {
            Self::Method(method) => method.as_str().to_owned(),
            Self::Tool(name) | Self::Operation(name) => name.as_str().to_owned(),
            Self::Target(name) => name.as_str().to_owned(),
            Self::Plan(digest) => digest.to_string(),
            Self::Mode(mode) => mode.as_str().to_owned(),
            Self::StepKind(kind) => kind.as_str().to_owned(),
            Self::Effect(effect) => effect.as_str().to_owned(),
            Self::Level(level) => level.as_str().to_owned(),
            Self::Class(class) => class.as_str().to_owned(),
            Self::Code(code) => code.as_str().to_owned(),
            Self::Count(count) => count.to_string(),
        }
    }
}

/// The MCP methods a call span can be named for. A method that is not listed
/// is recorded as `other`: its name is a caller's text and is not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McpMethod {
    /// `initialize`
    Initialize,
    /// `ping`
    Ping,
    /// `tools/list`
    ToolsList,
    /// `tools/call`
    ToolsCall,
    /// `resources/list`
    ResourcesList,
    /// `resources/templates/list`
    ResourcesTemplatesList,
    /// `resources/read`
    ResourcesRead,
    /// `prompts/list`
    PromptsList,
    /// `prompts/get`
    PromptsGet,
    /// Any other method.
    Other,
}

impl McpMethod {
    /// The method as the protocol writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Ping => "ping",
            Self::ToolsList => "tools/list",
            Self::ToolsCall => "tools/call",
            Self::ResourcesList => "resources/list",
            Self::ResourcesTemplatesList => "resources/templates/list",
            Self::ResourcesRead => "resources/read",
            Self::PromptsList => "prompts/list",
            Self::PromptsGet => "prompts/get",
            Self::Other => "other",
        }
    }

    /// The method a request names, or `Other` when it is not one of these.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        const ALL: [McpMethod; 9] = [
            McpMethod::Initialize,
            McpMethod::Ping,
            McpMethod::ToolsList,
            McpMethod::ToolsCall,
            McpMethod::ResourcesList,
            McpMethod::ResourcesTemplatesList,
            McpMethod::ResourcesRead,
            McpMethod::PromptsList,
            McpMethod::PromptsGet,
        ];
        ALL.into_iter()
            .find(|method| method.as_str() == name)
            .unwrap_or(Self::Other)
    }
}

/// How a call ended, for the metrics kept per tool version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    /// The call did what was asked.
    Success,
    /// The call stopped with a failure of this class. `mismatch` and `drift`
    /// are the two a rising rate of which is the early sign of a change in
    /// the system.
    Failed(FailureClass),
}
