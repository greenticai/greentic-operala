use clap::{Args, Parser, Subcommand};
use greentic_pack::builder::{
    FlowBundle, PACK_VERSION, PackBuilder, PackMeta, Provenance, Signing,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../extensions/bulk-ingest/mod.rs"]
mod bulk_ingest;

mod embedded_i18n {
    include!(concat!(env!("OUT_DIR"), "/embedded_i18n.rs"));
}

pub mod inference;

pub type OperalaResult<T> = Result<T, String>;

pub const ANSWERS_SCHEMA: &str = "greentic.operala.answers.v1";
pub const HANDOFF_SCHEMA: &str = "greentic.operala.handoff.v1";
pub const READINESS_SCHEMA: &str = "greentic.operala.readiness.v1";
pub const EXTENSION_RECONCILIATION: &str = "greentic.operala.reconciliation.v1";
pub const EXTENSION_BULK_INGEST: &str = "greentic.operala.bulk_ingest.v1";

const WIZARD_STAGES: &[&str] = &[
    "load_answers",
    "resolve_sorla",
    "load_extension",
    "validate_answers_schema",
    "run_extension_readiness",
    "maybe_generate_sorla_patch",
    "build_operala_handoff",
    "write_reports",
];

#[derive(Debug, Clone)]
pub enum ResolvedArtifact {
    LocalPath(PathBuf),
    Bytes(Vec<u8>),
    Json(Value),
    Yaml(serde_yaml::Value),
}

#[allow(async_fn_in_trait)]
pub trait ArtifactResolver {
    async fn resolve(
        &self,
        reference: &str,
        tenant: Option<&str>,
        team: Option<&str>,
    ) -> OperalaResult<ResolvedArtifact>;
}

#[derive(Debug, Clone)]
pub struct LocalCacheArtifactResolver {
    root: PathBuf,
}

impl LocalCacheArtifactResolver {
    pub fn from_env() -> Self {
        Self {
            root: env::var_os("OPERALA_DISTRIBUTOR_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".greentic/distributor")),
        }
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve_sync(
        &self,
        reference: &str,
        tenant: Option<&str>,
        team: Option<&str>,
    ) -> OperalaResult<ResolvedArtifact> {
        let path = if is_distributed_reference(reference) {
            self.root.join(distributed_cache_file_name(reference))
        } else {
            reference
                .strip_prefix("file://")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(reference))
        };
        if !path.exists() {
            return Err(format!(
                "artifact reference `{reference}` was accepted for tenant `{}` team `{}` but no cached artifact was found at {}; prefetch through greentic-distributor-client",
                tenant.unwrap_or(""),
                team.unwrap_or(""),
                path.display()
            ));
        }
        Ok(ResolvedArtifact::LocalPath(path))
    }
}

impl ArtifactResolver for LocalCacheArtifactResolver {
    async fn resolve(
        &self,
        reference: &str,
        tenant: Option<&str>,
        team: Option<&str>,
    ) -> OperalaResult<ResolvedArtifact> {
        self.resolve_sync(reference, tenant, team)
    }
}

#[derive(Debug, Parser)]
#[command(name = "greentic-operala")]
#[command(about = "Author OperaLa operational handoff artifacts")]
pub struct OperalaCli {
    #[command(subcommand)]
    pub command: OperalaCommand,
}

#[derive(Debug, Subcommand)]
pub enum OperalaCommand {
    Prompt(PromptArgs),
    Wizard(WizardArgs),
}

#[derive(Debug, Args)]
pub struct PromptArgs {
    #[arg(long)]
    pub sorla: String,
    #[arg(long)]
    pub locale: Option<String>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub tenant: Option<String>,
    #[arg(long)]
    pub team: Option<String>,
    /// LLM provider for inference (overrides GREENTIC_LLM_PROVIDER).
    #[arg(long, value_enum)]
    pub llm_provider: Option<greentic_llm::ProviderKind>,
    /// LLM model id (overrides GREENTIC_LLM_MODEL).
    #[arg(long)]
    pub llm_model: Option<String>,
    /// Force the deterministic keyword path even when an LLM is configured.
    #[arg(long, default_value_t = false)]
    pub no_llm: bool,
    /// Existing answers.json to update (update mode; requires an LLM).
    #[arg(long)]
    pub existing: Option<PathBuf>,
    /// Overwrite --existing in place instead of writing answers.updated.json.
    #[arg(long, default_value_t = false)]
    pub in_place: bool,
    pub prompt: String,
}

#[derive(Debug, Args)]
pub struct WizardArgs {
    #[arg(long)]
    pub schema: bool,
    #[arg(long)]
    pub answers: Option<String>,
    #[arg(long)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    File,
    Oci,
    Store,
    Repo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SorlaRef {
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gtpack_path: Option<PathBuf>,
    pub work_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalConfig {
    #[serde(default)]
    pub allow_sorla_patch_proposal: bool,
    #[serde(default)]
    pub apply_sorla_patch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperalaAnswers {
    pub schema: String,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_capability: Option<String>,
    pub extension: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    pub sorla: SorlaRef,
    pub outputs: OutputConfig,
    #[serde(default)]
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub capability_answers: CapabilityAnswers,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityAnswers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<ReconciliationAnswers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bulk_ingest: Option<BulkIngestAnswers>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationAnswers {
    pub name: String,
    pub source_event: String,
    pub expected_record: String,
    pub settlement_record: String,
    pub exception_record: String,
    #[serde(default)]
    pub input_modes: Vec<String>,
    pub source_fields: BTreeMap<String, String>,
    pub expected_fields: BTreeMap<String, String>,
    pub matching: MatchingConfig,
    pub exception_policy: BTreeMap<String, String>,
    pub actions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_endpoints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchingConfig {
    pub amount_tolerance: f64,
    pub date_window_days: u32,
    pub auto_match_threshold: u8,
    pub review_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkIngestAnswers {
    pub name: String,
    #[serde(default)]
    pub input_modes: Vec<String>,
    pub record_collections: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub expected_counts: BTreeMap<String, u64>,
    pub validation: BulkIngestValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkIngestValidation {
    #[serde(default = "default_true")]
    pub atomic: bool,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default = "default_true")]
    pub require_unique_ids: bool,
    #[serde(default = "default_true")]
    pub validate_references: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SorlaContract {
    pub source: SourceRef,
    pub source_digest: String,
    pub package_name: String,
    pub package_version: String,
    pub records: Vec<String>,
    pub events: Vec<String>,
    pub actions: Vec<String>,
    pub agent_endpoints: Vec<String>,
    pub raw_yaml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessReport {
    pub schema: String,
    pub capability: String,
    pub status: ReadinessStatus,
    pub found: BTreeMap<String, Value>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Ready,
    NeedsSorlaChanges,
    UnsafeOrAmbiguous,
}

pub trait OperaLaExtension {
    fn id(&self) -> &'static str;
    fn capability(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn qa_schema(&self) -> Value;
    /// JSON Schema for this extension's capability answers object. Handed to
    /// the LLM as the `emit_answers` tool schema. Guidance for the model —
    /// deterministic validation happens separately via serde + binding checks.
    fn answers_schema(&self) -> Value;
    fn analyse_sorla(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
    ) -> OperalaResult<ReadinessReport>;
    fn build_handoff(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
        readiness: &ReadinessReport,
    ) -> OperalaResult<OperaLaHandoff>;
}

static RECONCILIATION_EXTENSION: ReconciliationExtension = ReconciliationExtension;
static BULK_INGEST_EXTENSION: bulk_ingest::BulkIngestExtension = bulk_ingest::BulkIngestExtension;

pub struct ExtensionRegistry;

impl ExtensionRegistry {
    pub fn built_in() -> Self {
        Self
    }

    pub fn get(&self, id: &str) -> Option<&'static dyn OperaLaExtension> {
        match id {
            EXTENSION_RECONCILIATION => Some(&RECONCILIATION_EXTENSION),
            EXTENSION_BULK_INGEST => Some(&BULK_INGEST_EXTENSION),
            _ => None,
        }
    }

    pub fn all(&self) -> Vec<&'static dyn OperaLaExtension> {
        vec![&RECONCILIATION_EXTENSION, &BULK_INGEST_EXTENSION]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperaLaHandoff {
    pub schema: String,
    pub capability: String,
    pub extension: String,
    pub extension_version: String,
    pub tenant_required: bool,
    pub team_optional: bool,
    pub sorla: HandoffSorla,
    pub sorx: SorxBindingTemplate,
    pub bindings: Value,
    pub input_modes: Vec<String>,
    pub schemas: BTreeMap<String, Value>,
    pub flows: Vec<String>,
    pub ui: Vec<String>,
    pub tests: Vec<String>,
    pub readiness: ReadinessReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffSorla {
    pub source: SourceRef,
    pub source_digest: String,
    pub parser: String,
    pub required_schema: String,
    pub package_name: String,
    pub package_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SorxBindingTemplate {
    pub transport: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationBindings {
    pub source_event: String,
    pub expected_record: String,
    pub settlement_record: String,
    pub exception_record: String,
    pub actions: BTreeMap<String, String>,
    pub agent_endpoints: BTreeMap<String, String>,
    pub source_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardState {
    pub schema: String,
    pub status: String,
    pub stages: Vec<WizardStageState>,
    pub resumed_from_existing_state: bool,
    pub unresolved_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardStageState {
    pub id: String,
    pub status: String,
}

pub struct ReconciliationExtension;

impl OperaLaExtension for ReconciliationExtension {
    fn id(&self) -> &'static str {
        EXTENSION_RECONCILIATION
    }

    fn capability(&self) -> &'static str {
        "reconciliation"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn qa_schema(&self) -> Value {
        self.qa_schema_for_locale("en-GB")
    }

    fn answers_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "name", "source_event", "expected_record", "settlement_record",
                "exception_record", "input_modes", "source_fields", "expected_fields",
                "matching", "exception_policy", "actions", "agent_endpoints"
            ],
            "properties": {
                "name": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
                "source_event": { "type": "string", "description": "A SoRLa event id from catalog.events — the incoming observed payment event." },
                "expected_record": { "type": "string", "description": "A SoRLa record id from catalog.records — the expected obligation/invoice." },
                "settlement_record": { "type": "string", "description": "A SoRLa record id from catalog.records — stores the settled payment." },
                "exception_record": { "type": "string", "description": "A SoRLa record id from catalog.records — stores exceptions for manual review." },
                "input_modes": { "type": "array", "items": { "enum": ["single", "batch"] }, "minItems": 1 },
                "source_fields": { "type": "object", "additionalProperties": { "type": "string", "minLength": 1 } },
                "expected_fields": { "type": "object", "additionalProperties": { "type": "string", "minLength": 1 } },
                "matching": {
                    "type": "object",
                    "required": ["amount_tolerance", "date_window_days", "auto_match_threshold", "review_threshold"],
                    "properties": {
                        "amount_tolerance": { "type": "number", "minimum": 0 },
                        "date_window_days": { "type": "integer", "minimum": 0 },
                        "auto_match_threshold": { "type": "integer", "minimum": 0, "maximum": 100 },
                        "review_threshold": { "type": "integer", "minimum": 0, "maximum": 100 }
                    }
                },
                "exception_policy": { "type": "object", "additionalProperties": { "type": "string", "minLength": 1 } },
                "actions": { "type": "object", "description": "operation → SoRLa action id from catalog.actions", "additionalProperties": { "type": "string", "minLength": 1 } },
                "agent_endpoints": { "type": "object", "description": "operation → SoRLa agent endpoint id from catalog.agent_endpoints", "additionalProperties": { "type": "string", "minLength": 1 } }
            }
        })
    }

    fn analyse_sorla(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
    ) -> OperalaResult<ReadinessReport> {
        self.analyse_sorla_with_locale(sorla, answers, answers.locale.as_deref())
    }

    fn build_handoff(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
        readiness: &ReadinessReport,
    ) -> OperalaResult<OperaLaHandoff> {
        self.build_handoff_with_locale(sorla, answers, readiness)
    }
}

impl ReconciliationExtension {
    fn qa_schema_for_locale(&self, locale: &str) -> Value {
        json!({
            "schema": "greentic.qa.schema.v1",
            "flow": "operala.reconciliation",
            "sections": [
                {
                    "id": "reconciliation.source",
                    "title": t("operala.extension.reconciliation.source_event.label", Some(locale)),
                    "questions": [
                        {"id": "source_event", "label": t("operala.extension.reconciliation.source_event.label", Some(locale)), "kind": "select", "required": true},
                        {"id": "expected_record", "label": t("operala.extension.reconciliation.expected_record.label", Some(locale)), "kind": "select", "required": true},
                        {"id": "settlement_record", "label": t("operala.extension.reconciliation.settlement_record.label", Some(locale)), "kind": "select", "required": true},
                        {"id": "exception_record", "label": t("operala.extension.reconciliation.exception_record.label", Some(locale)), "kind": "select", "required": true}
                    ]
                },
                {
                    "id": "reconciliation.matching",
                    "title": t("operala.extension.reconciliation.matching.label", Some(locale)),
                    "questions": [
                        {"id": "amount_tolerance", "kind": "number", "required": true},
                        {"id": "date_window_days", "kind": "integer", "required": true},
                        {"id": "auto_match_threshold", "kind": "integer", "required": true},
                        {"id": "review_threshold", "kind": "integer", "required": true}
                    ]
                }
            ]
        })
    }

    fn analyse_sorla_with_locale(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
        _locale: Option<&str>,
    ) -> OperalaResult<ReadinessReport> {
        let recon = answers
            .capability_answers
            .reconciliation
            .as_ref()
            .ok_or_else(|| "missing capability_answers.reconciliation".to_string())?;
        let mut found = BTreeMap::new();
        let mut missing = Vec::new();
        let mut warnings = Vec::new();
        let mut ambiguities = Vec::new();

        check_named_or_candidates(
            &sorla.events,
            &recon.source_event,
            "source_event",
            &[
                "BankTransaction",
                "PaymentWebhook",
                "BankingTransactionImported",
            ],
            &mut found,
            &mut missing,
            &mut ambiguities,
        );
        check_named_or_candidates(
            &sorla.records,
            &recon.expected_record,
            "expected_record",
            &["RentObligation", "Invoice", "ExpectedPayment"],
            &mut found,
            &mut missing,
            &mut ambiguities,
        );
        check_named_or_candidates(
            &sorla.records,
            &recon.settlement_record,
            "settlement_record",
            &["Payment", "Receipt", "Allocation"],
            &mut found,
            &mut missing,
            &mut ambiguities,
        );
        check_named_or_candidates(
            &sorla.records,
            &recon.exception_record,
            "exception_record",
            &["ReconciliationCase", "ManualReviewCase", "PaymentException"],
            &mut found,
            &mut missing,
            &mut ambiguities,
        );

        for (role, action) in &recon.actions {
            if action.trim().is_empty() {
                ambiguities.push(format!(
                    "action `{role}` requires an explicit SoRLa action id"
                ));
            } else if sorla.actions.iter().any(|candidate| candidate == action) {
                found.insert(format!("action.{role}"), Value::String(action.clone()));
            } else {
                missing.push(format!("action `{action}` for `{role}`"));
            }
        }

        for (role, endpoint) in &recon.agent_endpoints {
            if sorla
                .agent_endpoints
                .iter()
                .any(|candidate| candidate == endpoint)
            {
                found.insert(
                    format!("agent_endpoint.{role}"),
                    Value::String(endpoint.clone()),
                );
            } else {
                warnings.push(format!(
                    "agent endpoint `{endpoint}` for `{role}` is not declared; action handoff metadata can still be generated"
                ));
            }
        }

        if recon.matching.auto_match_threshold < recon.matching.review_threshold {
            missing.push(
                "matching.auto_match_threshold must be greater than or equal to review_threshold"
                    .to_string(),
            );
        }
        warnings.extend(ambiguities.iter().cloned());

        let status = if !ambiguities.is_empty() {
            ReadinessStatus::UnsafeOrAmbiguous
        } else if missing.is_empty() {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NeedsSorlaChanges
        };
        let summary = match status {
            ReadinessStatus::Ready => t("operala.readiness.reconciliation.ready", _locale),
            ReadinessStatus::NeedsSorlaChanges => t(
                "operala.readiness.reconciliation.needs_sorla_changes",
                _locale,
            ),
            ReadinessStatus::UnsafeOrAmbiguous => t(
                "operala.readiness.reconciliation.unsafe_or_ambiguous",
                _locale,
            ),
        };

        Ok(ReadinessReport {
            schema: READINESS_SCHEMA.to_string(),
            capability: "reconciliation".to_string(),
            status,
            found,
            missing,
            warnings,
            summary,
        })
    }

    fn build_handoff_with_locale(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
        readiness: &ReadinessReport,
    ) -> OperalaResult<OperaLaHandoff> {
        let recon = answers
            .capability_answers
            .reconciliation
            .as_ref()
            .ok_or_else(|| "missing capability_answers.reconciliation".to_string())?;
        let mut schemas = BTreeMap::new();
        schemas.insert("bank-transaction".to_string(), bank_transaction_schema());
        schemas.insert(
            "daily-bank-transactions".to_string(),
            daily_bank_transactions_schema(),
        );

        Ok(OperaLaHandoff {
            schema: HANDOFF_SCHEMA.to_string(),
            capability: "reconciliation".to_string(),
            extension: self.id().to_string(),
            extension_version: self.version().to_string(),
            tenant_required: true,
            team_optional: true,
            sorla: HandoffSorla {
                source: sorla.source.clone(),
                source_digest: sorla.source_digest.clone(),
                parser: "greentic-sorla-lib".to_string(),
                required_schema: "greentic.sorla.v0.2".to_string(),
                package_name: sorla.package_name.clone(),
                package_version: sorla.package_version.clone(),
            },
            sorx: SorxBindingTemplate {
                transport: "http".to_string(),
                url: "runtime-provided".to_string(),
            },
            bindings: json!({
                "source_event": recon.source_event.clone(),
                "expected_record": recon.expected_record.clone(),
                "settlement_record": recon.settlement_record.clone(),
                "exception_record": recon.exception_record.clone(),
                "actions": recon.actions.clone(),
                "agent_endpoints": recon.agent_endpoints.clone(),
                "source_digest": sorla.source_digest.clone(),
            }),
            input_modes: if recon.input_modes.is_empty() {
                vec!["single".to_string(), "batch".to_string()]
            } else {
                recon.input_modes.clone()
            },
            schemas,
            flows: vec![
                "ingest-transaction.flow.yaml".to_string(),
                "ingest-daily-transactions.flow.yaml".to_string(),
                "reconcile-one.flow.yaml".to_string(),
            ],
            ui: vec!["reconciliation-exception.card.json".to_string()],
            tests: vec![
                "one-transaction.json".to_string(),
                "daily-transactions.json".to_string(),
                "expected-decisions.json".to_string(),
            ],
            readiness: readiness.clone(),
        })
    }
}

fn check_named_or_candidates(
    candidates: &[String],
    expected: &str,
    key: &str,
    plausible: &[&str],
    found: &mut BTreeMap<String, Value>,
    missing: &mut Vec<String>,
    ambiguities: &mut Vec<String>,
) {
    if expected.trim().is_empty() {
        let matches = candidates
            .iter()
            .filter(|candidate| plausible.iter().any(|name| name == &candidate.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [one] => {
                found.insert(key.to_string(), Value::String(one.clone()));
            }
            [] => missing.push(format!("{key} requires an explicit SoRLa id")),
            many => ambiguities.push(format!(
                "{key} is ambiguous; choose one of {}",
                many.join(", ")
            )),
        }
    } else if candidates.iter().any(|candidate| candidate == expected) {
        found.insert(key.to_string(), Value::String(expected.to_string()));
    } else {
        missing.push(format!("{key} `{expected}`"));
    }
}

pub fn run_operala_cli() -> std::process::ExitCode {
    let args = env::args_os().collect::<Vec<_>>();
    if let Some(help) = localized_operala_help_for_args(&args) {
        println!("{help}");
        return std::process::ExitCode::SUCCESS;
    }
    match run_operala(OperalaCli::parse_from(args)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("greentic-operala: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn run_operala(cli: OperalaCli) -> OperalaResult<()> {
    match cli.command {
        OperalaCommand::Prompt(args) => {
            let resolved = inference::resolve_llm_request_from_process_env(&args)?;
            let llm_runtime = match &resolved {
                Some(resolved) => Some(inference::LlmRuntime::build(resolved)?),
                None => {
                    if !args.no_llm {
                        eprintln!(
                            "greentic-operala: note: no LLM configured; using the deterministic keyword path (set GREENTIC_LLM_PROVIDER/GREENTIC_LLM_MODEL or pass --llm-provider/--llm-model to enable LLM inference)"
                        );
                    }
                    None
                }
            };
            let llm_ref = llm_runtime
                .as_ref()
                .map(|runtime| runtime as &dyn inference::ChatFn);
            if let Some(existing_path) = &args.existing {
                let Some(chat) = llm_ref else {
                    return Err(
                        "--existing (update mode) requires an LLM; pass --llm-provider/--llm-model or set GREENTIC_LLM_PROVIDER/GREENTIC_LLM_MODEL"
                            .to_string(),
                    );
                };
                let raw = fs::read_to_string(existing_path)
                    .map_err(|err| format!("failed to read {}: {err}", existing_path.display()))?;
                let existing: OperalaAnswers = serde_json::from_str(&raw).map_err(to_string)?;
                let outcome =
                    inference::update_answers(chat, &existing, &args.sorla, &args.prompt)?;
                let output = match (&args.output, args.in_place) {
                    (Some(output), _) => output.clone(),
                    (None, true) => existing_path.clone(),
                    (None, false) => existing_path.with_file_name("answers.updated.json"),
                };
                write_json_file(&output, &outcome.answers)?;
                if outcome.diff.is_empty() {
                    println!("no changes");
                } else {
                    println!("{}", inference::diff::format_diff(&outcome.diff));
                }
                println!("updated answers written to {}", output.display());
                return Ok(());
            }
            let answers = prompt_answers_with_llm(&args, llm_ref)?;
            let output = args
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from("answers.json"));
            write_json_file(&output, &answers)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&answers).map_err(to_string)?
            );
            Ok(())
        }
        OperalaCommand::Wizard(args) => {
            if args.schema {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&wizard_schema(args.locale.as_deref()))
                        .map_err(to_string)?
                );
                return Ok(());
            }
            let answers_ref = args
                .answers
                .as_deref()
                .ok_or_else(|| "wizard requires --schema or --answers <ref>".to_string())?;
            let answers = load_answers(answers_ref)?;
            let output = run_wizard(&answers)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&output).map_err(to_string)?
            );
            Ok(())
        }
    }
}

pub fn prompt_answers(args: &PromptArgs) -> OperalaResult<OperalaAnswers> {
    prompt_answers_with_llm(args, None)
}

pub fn prompt_answers_with_llm(
    args: &PromptArgs,
    llm: Option<&dyn inference::ChatFn>,
) -> OperalaResult<OperalaAnswers> {
    let sorla = load_sorla_contract(&SourceRef {
        kind: SourceKind::File,
        uri: args.sorla.clone(),
        digest: None,
    })?;
    let capability = detect_capability(&args.prompt, llm)?;

    let (extension, reconciliation, bulk_ingest, output_name) = match (capability, llm) {
        ("reconciliation", Some(chat)) => {
            let value = inference::infer_capability_answers(
                chat,
                EXTENSION_RECONCILIATION,
                &RECONCILIATION_EXTENSION.answers_schema(),
                &sorla,
                &args.prompt,
                None,
            )?;
            let reconciliation: ReconciliationAnswers =
                serde_json::from_value(value).map_err(to_string)?;
            (
                EXTENSION_RECONCILIATION.to_string(),
                Some(reconciliation.clone()),
                None,
                reconciliation.name.clone(),
            )
        }
        ("bulk_ingest", Some(chat)) => {
            let value = inference::infer_capability_answers(
                chat,
                EXTENSION_BULK_INGEST,
                &BULK_INGEST_EXTENSION.answers_schema(),
                &sorla,
                &args.prompt,
                None,
            )?;
            let bulk: BulkIngestAnswers = serde_json::from_value(value).map_err(to_string)?;
            (
                EXTENSION_BULK_INGEST.to_string(),
                None,
                Some(bulk.clone()),
                bulk.name.clone(),
            )
        }
        ("bulk_ingest", None) => {
            let bulk = bulk_ingest::infer_answers(&sorla, &args.prompt);
            (
                EXTENSION_BULK_INGEST.to_string(),
                None,
                Some(bulk.clone()),
                bulk.name.clone(),
            )
        }
        ("reconciliation", None) => {
            let reconciliation = infer_reconciliation_answers(&sorla)?;
            (
                EXTENSION_RECONCILIATION.to_string(),
                Some(reconciliation.clone()),
                None,
                reconciliation.name.clone(),
            )
        }
        (other, None) => {
            return Err(format!("unsupported capability '{other}'"));
        }
        (other, Some(_)) => {
            return Err(format!("unsupported capability '{other}'"));
        }
    };
    let work_dir = PathBuf::from(format!("target/operala/{output_name}"));
    let gtpack_file_name = format!("{}.gtpack", output_name.replace('_', "-"));
    Ok(OperalaAnswers {
        schema: ANSWERS_SCHEMA.to_string(),
        intent: args.prompt.clone(),
        detected_capability: Some(capability.to_string()),
        extension,
        locale: args.locale.clone(),
        tenant: args.tenant.clone(),
        team: args.team.clone(),
        sorla: SorlaRef {
            source: SourceRef {
                kind: SourceKind::File,
                uri: args.sorla.clone(),
                digest: Some(sorla.source_digest.clone()),
            },
            expected_schema: Some("greentic.sorla.v0.2".to_string()),
        },
        outputs: OutputConfig {
            handoff_path: Some(work_dir.join("operala-handoff.json")),
            gtpack_path: Some(work_dir.join(gtpack_file_name)),
            work_dir,
        },
        approval: ApprovalConfig {
            allow_sorla_patch_proposal: true,
            apply_sorla_patch: false,
        },
        capability_answers: CapabilityAnswers {
            reconciliation,
            bulk_ingest,
        },
        assumptions: Vec::new(),
    })
}

fn detect_capability(
    prompt: &str,
    llm: Option<&dyn inference::ChatFn>,
) -> OperalaResult<&'static str> {
    let lower_prompt = prompt.to_ascii_lowercase();
    if lower_prompt.contains("bulk ingest")
        || lower_prompt.contains("bulk upload")
        || lower_prompt.contains("batch upload")
        || lower_prompt.contains("upload operation")
    {
        return Ok("bulk_ingest");
    }
    if lower_prompt.contains("reconcil")
        || lower_prompt.contains("bank transaction")
        || lower_prompt.contains("rent payment")
        || lower_prompt.contains("invoice payment")
    {
        return Ok("reconciliation");
    }
    if let Some(chat) = llm
        && let Some(capability) = inference::classify_capability(chat, prompt)?
    {
        return Ok(match capability.as_str() {
            "reconciliation" => "reconciliation",
            "bulk_ingest" => "bulk_ingest",
            _ => {
                return Err(follow_up_required(&format!(
                    "the LLM classified this as '{capability}', which OperaLa does not author; which operational capability should it use for this SoRLa contract?"
                )));
            }
        });
    }
    Err(follow_up_required(
        "Which operational capability should OperaLa author for this SoRLa contract?",
    ))
}

pub fn wizard_schema(locale: Option<&str>) -> Value {
    let locale = normalized_locale(locale);
    let registry = ExtensionRegistry::built_in();
    json!({
        "schema": "greentic.qa.schema.v1",
        "generated_by": greentic_qa_engine(),
        "product": "operala",
            "locale": locale,
            "fallback_locale": "en-GB",
            "i18n": {
            "text_direction": text_direction(&locale),
            "operala.cli.about": t("operala.cli.about", Some(&locale)),
            "operala.cli.prompt.about": t("operala.cli.prompt.about", Some(&locale)),
            "operala.cli.wizard.about": t("operala.cli.wizard.about", Some(&locale))
        },
        "answers_schema": ANSWERS_SCHEMA,
        "extensions": registry
            .all()
            .into_iter()
            .map(|extension| {
                if extension.id() == EXTENSION_RECONCILIATION {
                    RECONCILIATION_EXTENSION.qa_schema_for_locale(&locale)
                } else {
                    extension.qa_schema()
                }
            })
            .collect::<Vec<_>>()
    })
}

pub fn load_answers(reference: &str) -> OperalaResult<OperalaAnswers> {
    let path = resolve_local_path(reference, None, None)?;
    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read answers {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse answers {}: {err}", path.display()))
}

pub fn run_wizard(answers: &OperalaAnswers) -> OperalaResult<Value> {
    let state_path = answers.outputs.work_dir.join("operala.state.json");
    let resumed = state_path.exists();
    write_wizard_state(&state_path, "running", &["load_answers"], resumed, &[])?;
    validate_answers(answers)?;
    write_wizard_state(
        &state_path,
        "running",
        &["load_answers", "validate_answers_schema"],
        resumed,
        &[],
    )?;
    let sorla = load_sorla_contract(&answers.sorla.source)?;
    write_wizard_state(
        &state_path,
        "running",
        &["load_answers", "validate_answers_schema", "resolve_sorla"],
        resumed,
        &[],
    )?;
    let registry = ExtensionRegistry::built_in();
    let extension = registry.get(&answers.extension).ok_or_else(|| {
        format!(
            "{}: `{}`",
            t(
                "operala.validation.unknown_extension",
                answers.locale.as_deref()
            ),
            answers.extension
        )
    })?;
    write_wizard_state(
        &state_path,
        "running",
        &[
            "load_answers",
            "validate_answers_schema",
            "resolve_sorla",
            "load_extension",
        ],
        resumed,
        &[],
    )?;
    let readiness = extension.analyse_sorla(&sorla, answers)?;
    write_wizard_state(
        &state_path,
        "running",
        &[
            "load_answers",
            "validate_answers_schema",
            "resolve_sorla",
            "load_extension",
            "run_extension_readiness",
        ],
        resumed,
        &readiness.missing,
    )?;
    let handoff = extension.build_handoff(&sorla, answers, &readiness)?;

    fs::create_dir_all(&answers.outputs.work_dir).map_err(|err| {
        format!(
            "failed to create work dir {}: {err}",
            answers.outputs.work_dir.display()
        )
    })?;
    let readiness_path = answers.outputs.work_dir.join("readiness.report.yaml");
    write_yaml_file(&readiness_path, &readiness)?;
    let patch_path = answers.outputs.work_dir.join("sorla.patch.json");
    let patch_proposal = sorla_patch_proposal(&readiness, &sorla)?;
    write_json_file(&patch_path, &patch_proposal)?;
    write_wizard_state(
        &state_path,
        "running",
        &[
            "load_answers",
            "validate_answers_schema",
            "resolve_sorla",
            "load_extension",
            "run_extension_readiness",
            "maybe_generate_sorla_patch",
        ],
        resumed,
        &readiness.missing,
    )?;
    let preview_path = answers
        .outputs
        .work_dir
        .join("operala.handoff.preview.json");
    write_json_file(&preview_path, &handoff)?;
    let handoff_path = answers
        .outputs
        .handoff_path
        .clone()
        .unwrap_or_else(|| answers.outputs.work_dir.join("operala-handoff.json"));
    write_json_file(&handoff_path, &handoff)?;
    write_handoff_assets(&answers.outputs.work_dir, &handoff)?;

    let work_dir_gtpack_path = answers.outputs.work_dir.join(format!(
        "{}.gtpack",
        capability_output_name(answers, &handoff).replace('_', "-")
    ));
    let configured_gtpack_path = answers.outputs.gtpack_path.clone();
    let primary_gtpack_path = configured_gtpack_path
        .clone()
        .unwrap_or_else(|| work_dir_gtpack_path.clone());
    if primary_gtpack_path == work_dir_gtpack_path {
        write_operala_gtpack(&primary_gtpack_path, &handoff)?;
    } else {
        write_operala_gtpack(&primary_gtpack_path, &handoff)?;
        write_operala_gtpack(&work_dir_gtpack_path, &handoff)?;
    }
    write_wizard_state(
        &state_path,
        "running",
        &[
            "load_answers",
            "validate_answers_schema",
            "resolve_sorla",
            "load_extension",
            "run_extension_readiness",
            "maybe_generate_sorla_patch",
            "build_operala_handoff",
        ],
        resumed,
        &readiness.missing,
    )?;

    let lock = build_lock(answers, &sorla, &handoff)?;
    let lock_path = answers.outputs.work_dir.join("operala.build.lock");
    write_json_file(&lock_path, &lock)?;
    let summary_path = answers.outputs.work_dir.join("build.summary.md");
    fs::write(&summary_path, build_summary(&readiness, &handoff))
        .map_err(|err| format!("failed to write {}: {err}", summary_path.display()))?;
    write_wizard_state(
        &state_path,
        "complete",
        WIZARD_STAGES,
        resumed,
        &readiness.missing,
    )?;

    Ok(json!({
        "schema": "greentic.operala.wizard-result.v1",
        "status": if readiness.status == ReadinessStatus::Ready { "ready" } else { "needs_attention" },
        "work_dir": answers.outputs.work_dir,
        "handoff_path": handoff_path,
        "readiness_path": readiness_path,
        "patch_path": patch_path,
        "lock_path": lock_path,
        "state_path": state_path,
        "summary_path": summary_path,
        "gtpack_path": work_dir_gtpack_path,
        "configured_gtpack_path": configured_gtpack_path,
        "warnings": readiness.warnings,
        "missing": readiness.missing
    }))
}

pub fn validate_answers(answers: &OperalaAnswers) -> OperalaResult<()> {
    if answers.schema != ANSWERS_SCHEMA {
        return Err(format!(
            "{}: schema",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.intent.trim().is_empty() {
        return Err(format!(
            "{}: intent",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.extension.trim().is_empty() {
        return Err(format!(
            "{}: extension",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.sorla.source.uri.trim().is_empty() {
        return Err(format!(
            "{}: sorla.source.uri",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.outputs.work_dir.as_os_str().is_empty() {
        return Err(format!(
            "{}: outputs.work_dir",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.outputs.handoff_path.is_none() && answers.outputs.gtpack_path.is_none() {
        return Err(format!(
            "{}: outputs.handoff_path",
            t(
                "operala.validation.missing_field",
                answers.locale.as_deref()
            )
        ));
    }
    if answers.tenant.as_deref().unwrap_or("").trim().is_empty() {
        return Err(t(
            "operala.validation.tenant_required",
            answers.locale.as_deref(),
        )
        .to_string());
    }
    if answers.approval.apply_sorla_patch {
        return Err(t(
            "operala.validation.apply_patch_forbidden",
            answers.locale.as_deref(),
        )
        .to_string());
    }
    if answers.extension == EXTENSION_RECONCILIATION {
        let recon = answers
            .capability_answers
            .reconciliation
            .as_ref()
            .ok_or_else(|| "capability_answers.reconciliation is required".to_string())?;
        if recon.input_modes.is_empty() {
            return Err("capability_answers.reconciliation.input_modes is required".to_string());
        }
        for mode in &recon.input_modes {
            if mode != "single" && mode != "batch" {
                return Err(format!(
                    "capability_answers.reconciliation.input_modes contains unsupported mode `{mode}`"
                ));
            }
        }
        require_map_keys(
            "capability_answers.reconciliation.source_fields",
            &recon.source_fields,
            &["external_id", "amount", "date", "reference", "currency"],
        )?;
        require_map_keys(
            "capability_answers.reconciliation.expected_fields",
            &recon.expected_fields,
            &["id", "amount", "due_date", "reference", "status"],
        )?;
        require_map_keys(
            "capability_answers.reconciliation.exception_policy",
            &recon.exception_policy,
            &[
                "partial_payment",
                "ambiguous_match",
                "unmatched",
                "duplicate_possible",
            ],
        )?;
        require_map_keys(
            "capability_answers.reconciliation.actions",
            &recon.actions,
            &[
                "create_settlement",
                "mark_paid",
                "mark_partially_paid",
                "create_exception",
            ],
        )?;
        if recon.matching.auto_match_threshold > 100 || recon.matching.review_threshold > 100 {
            return Err("matching thresholds must be percentages from 0 to 100".to_string());
        }
        if recon.matching.auto_match_threshold < recon.matching.review_threshold {
            return Err(
                "matching.auto_match_threshold must be greater than or equal to review_threshold"
                    .to_string(),
            );
        }
    } else if answers.extension == EXTENSION_BULK_INGEST {
        let bulk = answers
            .capability_answers
            .bulk_ingest
            .as_ref()
            .ok_or_else(|| "capability_answers.bulk_ingest is required".to_string())?;
        if bulk.name.trim().is_empty() {
            return Err("capability_answers.bulk_ingest.name is required".to_string());
        }
        if bulk.record_collections.is_empty() {
            return Err(
                "capability_answers.bulk_ingest.record_collections is required".to_string(),
            );
        }
        for mode in &bulk.input_modes {
            if mode != "batch" {
                return Err(format!(
                    "capability_answers.bulk_ingest.input_modes contains unsupported mode `{mode}`"
                ));
            }
        }
    }
    Ok(())
}

fn require_map_keys(
    label: &str,
    values: &BTreeMap<String, String>,
    required: &[&str],
) -> OperalaResult<()> {
    for key in required {
        if values.get(*key).is_none_or(|value| value.trim().is_empty()) {
            return Err(format!("{label}.{key} is required"));
        }
    }
    Ok(())
}

pub fn load_sorla_contract(source: &SourceRef) -> OperalaResult<SorlaContract> {
    let path = resolve_local_path(&source.uri, None, None)?;
    let raw_yaml = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read SoRLa source {}: {err}", path.display()))?;
    let actual_digest = format!("sha256:{}", sha256_hex(raw_yaml.as_bytes()));
    verify_reference_digest(&source.uri, source.digest.as_deref(), &actual_digest)?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&raw_yaml)
        .map_err(|err| format!("failed to parse SoRLa YAML {}: {err}", path.display()))?;
    let package = yaml
        .get("package")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| "SoRLa source must contain package".to_string())?;
    let package_name = yaml_string(package, "name").unwrap_or_else(|| "unknown".to_string());
    let package_version = yaml_string(package, "version").unwrap_or_else(|| "0.1.0".to_string());
    Ok(SorlaContract {
        source: SourceRef {
            kind: source.kind.clone(),
            uri: source.uri.clone(),
            digest: Some(actual_digest.clone()),
        },
        source_digest: actual_digest,
        package_name,
        package_version,
        records: yaml_named_list(&yaml, "records"),
        events: yaml_named_list(&yaml, "events"),
        actions: yaml_named_list(&yaml, "actions"),
        agent_endpoints: yaml_id_list(&yaml, "agent_endpoints"),
        raw_yaml,
    })
}

fn infer_reconciliation_answers(sorla: &SorlaContract) -> OperalaResult<ReconciliationAnswers> {
    let source_event =
        pick(&sorla.events, &["BankTransaction", "PaymentWebhook"]).ok_or_else(|| {
            follow_up_required(
                "Which SoRLa event should be used as the incoming observed payment event?",
            )
        })?;
    let expected_record = pick(
        &sorla.records,
        &["RentObligation", "Invoice", "ExpectedPayment"],
    )
    .ok_or_else(|| {
        follow_up_required(
            "Which SoRLa record represents the expected obligation or invoice to match against?",
        )
    })?;
    let settlement_record = pick(&sorla.records, &["Payment", "Receipt", "Allocation"])
        .ok_or_else(|| {
            follow_up_required("Which SoRLa record should store the settled payment or allocation?")
        })?;
    let exception_record = pick(
        &sorla.records,
        &["ReconciliationCase", "ManualReviewCase", "PaymentException"],
    )
    .ok_or_else(|| {
        follow_up_required(
            "Which SoRLa record should store reconciliation exceptions for manual review?",
        )
    })?;

    Ok(ReconciliationAnswers {
        name: "tenancy_rent_reconciliation".to_string(),
        source_event,
        expected_record,
        settlement_record,
        exception_record,
        input_modes: vec!["single".to_string(), "batch".to_string()],
        source_fields: map([
            ("external_id", "transaction_id"),
            ("amount", "amount"),
            ("date", "booked_at"),
            ("reference", "reference"),
            ("currency", "currency"),
        ]),
        expected_fields: map([
            ("id", "obligation_id"),
            ("amount", "expected_amount"),
            ("due_date", "due_date"),
            ("reference", "tenancy_reference"),
            ("status", "status"),
        ]),
        matching: MatchingConfig {
            amount_tolerance: 2.0,
            date_window_days: 7,
            auto_match_threshold: 85,
            review_threshold: 50,
        },
        exception_policy: map([
            ("partial_payment", "create_case"),
            ("ambiguous_match", "manual_allocation"),
            ("unmatched", "unallocated_cash_case"),
            ("duplicate_possible", "manual_review"),
        ]),
        actions: map([
            ("create_settlement", "create_payment"),
            ("mark_paid", "mark_obligation_paid"),
            ("mark_partially_paid", "mark_obligation_partially_paid"),
            ("create_exception", "create_reconciliation_case"),
        ]),
        agent_endpoints: map([
            ("create_settlement", "create_payment"),
            ("mark_paid", "mark_obligation_paid"),
            ("mark_partially_paid", "mark_obligation_partially_paid"),
            ("create_exception", "create_reconciliation_case"),
        ]),
    })
}

fn follow_up_required(question: &str) -> String {
    format!("follow-up required: {question}")
}

fn sorla_patch_proposal(
    readiness: &ReadinessReport,
    sorla: &SorlaContract,
) -> OperalaResult<Value> {
    let mut operations = Vec::new();
    let mut unsupported_missing = Vec::new();
    for missing in &readiness.missing {
        if missing.contains("settlement_record") {
            let record_name = missing_concept_name(missing).unwrap_or("Payment");
            operations.push(add_payment_record_operation(record_name));
        } else if missing.contains("exception_record") {
            let record_name = missing_concept_name(missing).unwrap_or("ReconciliationCase");
            operations.push(add_reconciliation_case_record_operation(record_name));
        } else if missing.contains("action `") || missing.contains("agent endpoint `") {
            unsupported_missing.push(missing.clone());
        }
    }
    let status = if operations.is_empty() {
        if readiness.missing.is_empty() {
            "not_needed"
        } else {
            "manual_required"
        }
    } else {
        "proposed"
    };
    let proposal = json!({
        "schema": "greentic.sorla.patch.v1",
        "source": {
            "kind": "sorla-yaml",
            "path": sorla.source.uri,
            "base_hash": sorla.source_digest
        },
        "author": {
            "id": "greentic-operala",
            "name": "OperaLa",
            "kind": "tool"
        },
        "reason": "required_for_operala_reconciliation",
        "intent": "Add missing SoRLa records required for OperaLa reconciliation. Proposal only; not applied by OperaLa.",
        "status": status,
        "missing": readiness.missing,
        "unsupported_missing": unsupported_missing,
        "operations": operations
    });
    validate_sorla_patch_proposal(&proposal)?;
    Ok(proposal)
}

fn add_payment_record_operation(record_name: &str) -> Value {
    let patch_name = patch_record_name(record_name);
    json!({
        "op": "add_record",
        "record": {
            "name": patch_name,
            "requested_name": record_name,
            "source": "native",
            "fields": [
                {"name": "payment_id", "type": "uuid", "required": true, "rules": {"unique": true}},
                {"name": "obligation_id", "type": "uuid", "required": true, "references": {"record": "RentObligation", "field": "obligation_id"}},
                {"name": "amount", "type": "decimal", "required": true, "rules": {"min": 0, "precision": 12, "scale": 2}},
                {"name": "received_at", "type": "datetime", "required": true},
                {"name": "source_event_id", "type": "string", "required": true}
            ]
        }
    })
}

fn add_reconciliation_case_record_operation(record_name: &str) -> Value {
    let patch_name = patch_record_name(record_name);
    json!({
        "op": "add_record",
        "record": {
            "name": patch_name,
            "requested_name": record_name,
            "source": "native",
            "fields": [
                {"name": "case_id", "type": "uuid", "required": true, "rules": {"unique": true}},
                {"name": "source_event_id", "type": "string", "required": true},
                {"name": "expected_record_id", "type": "uuid", "required": true},
                {"name": "reason", "type": "string", "required": true},
                {"name": "status", "type": "string", "required": true}
            ]
        }
    })
}

fn validate_sorla_patch_proposal(proposal: &Value) -> OperalaResult<()> {
    if proposal["schema"] != "greentic.sorla.patch.v1" {
        return Err("SoRLa patch proposal has unsupported schema".to_string());
    }
    if proposal["source"]["kind"] != "sorla-yaml" {
        return Err("SoRLa patch proposal source.kind must be sorla-yaml".to_string());
    }
    let base_hash = proposal["source"]["base_hash"]
        .as_str()
        .ok_or_else(|| "SoRLa patch proposal source.base_hash is required".to_string())?;
    if !base_hash.starts_with("sha256:") {
        return Err("SoRLa patch proposal source.base_hash must be a sha256 digest".to_string());
    }
    let operations = proposal["operations"]
        .as_array()
        .ok_or_else(|| "SoRLa patch proposal operations must be an array".to_string())?;
    for operation in operations {
        if operation["op"] != "add_record" {
            return Err("OperaLa may only propose additive SoRLa record patches".to_string());
        }
        let record = operation["record"]
            .as_object()
            .ok_or_else(|| "add_record operation requires record".to_string())?;
        if record
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return Err("add_record operation requires record.name".to_string());
        }
        let record_name = record.get("name").and_then(Value::as_str).unwrap_or("");
        if !is_sorla_patch_identifier(record_name) {
            return Err(format!(
                "add_record record.name `{record_name}` is not a valid SoRLa patch identifier"
            ));
        }
        let fields = record
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| "add_record operation requires record.fields".to_string())?;
        if fields.is_empty() {
            return Err("add_record operation requires at least one field".to_string());
        }
        for field in fields {
            if field
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
                || field
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty()
            {
                return Err("add_record field requires name and type".to_string());
            }
        }
    }
    Ok(())
}

fn missing_concept_name(message: &str) -> Option<&str> {
    let (_, rest) = message.split_once('`')?;
    let (name, _) = rest.split_once('`')?;
    Some(name)
}

fn patch_record_name(name: &str) -> String {
    let mut out = String::new();
    let mut previous_was_separator = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !previous_was_separator && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !out.ends_with('_') {
            out.push('_');
            previous_was_separator = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
    {
        trimmed
    } else {
        format!("record_{trimmed}")
    }
}

fn is_sorla_patch_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn build_lock(
    answers: &OperalaAnswers,
    sorla: &SorlaContract,
    handoff: &OperaLaHandoff,
) -> OperalaResult<Value> {
    let answers_bytes = serde_json::to_vec(answers).map_err(to_string)?;
    let handoff_bytes = serde_json::to_vec(handoff).map_err(to_string)?;
    Ok(json!({
        "schema": "greentic.operala.build-lock.v1",
        "answers_digest": format!("sha256:{}", sha256_hex(&answers_bytes)),
        "resolved_answers_digest": format!("sha256:{}", sha256_hex(&answers_bytes)),
        "sorla_source_digest": sorla.source_digest,
        "extension": handoff.extension,
        "extension_version": handoff.extension_version,
        "handoff_digest": format!("sha256:{}", sha256_hex(&handoff_bytes))
    }))
}

fn build_summary(readiness: &ReadinessReport, handoff: &OperaLaHandoff) -> String {
    let locale = None;
    let unresolved = if readiness.missing.is_empty() {
        format!("{}\n", t("operala.summary.unresolved_none", locale))
    } else {
        format!(
            "{}:\n{}\n",
            t("operala.summary.unresolved", locale),
            readiness
                .missing
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    format!(
        "# {}\n\nCapability: {}\nStatus: {:?}\nExtension: {}\n\n{}\n\nSoRLa patch proposal: {}\n\n{}",
        t("operala.summary.title", locale),
        handoff.capability,
        readiness.status,
        handoff.extension,
        readiness.summary,
        if readiness.missing.is_empty() {
            "not needed"
        } else {
            "proposed, not applied"
        },
        unresolved
    )
}

fn capability_output_name(answers: &OperalaAnswers, handoff: &OperaLaHandoff) -> String {
    answers
        .capability_answers
        .bulk_ingest
        .as_ref()
        .map(|bulk| bulk.name.clone())
        .or_else(|| {
            answers
                .capability_answers
                .reconciliation
                .as_ref()
                .map(|recon| recon.name.clone())
        })
        .unwrap_or_else(|| handoff.capability.clone())
}

fn write_wizard_state(
    path: &Path,
    status: &str,
    completed: &[&str],
    resumed: bool,
    unresolved_questions: &[String],
) -> OperalaResult<()> {
    let stages = WIZARD_STAGES
        .iter()
        .map(|stage| WizardStageState {
            id: (*stage).to_string(),
            status: if completed.contains(stage) {
                "complete".to_string()
            } else {
                "pending".to_string()
            },
        })
        .collect::<Vec<_>>();
    write_json_file(
        path,
        &WizardState {
            schema: "greentic.operala.wizard-state.v1".to_string(),
            status: status.to_string(),
            stages,
            resumed_from_existing_state: resumed,
            unresolved_questions: unresolved_questions.to_vec(),
        },
    )
}

fn write_handoff_assets(work_dir: &Path, handoff: &OperaLaHandoff) -> OperalaResult<()> {
    write_yaml_file(work_dir.join("operala.yaml"), handoff)?;
    write_json_file(
        work_dir
            .join("capability")
            .join(format!("{}.json", handoff.capability)),
        &json!({
            "schema": format!("greentic.operala.capability.{}.v1", handoff.capability),
            "handoff_schema": handoff.schema,
            "sorla_source_digest": handoff.sorla.source_digest,
            "bindings": handoff.bindings,
            "input_modes": handoff.input_modes,
            "flows": handoff.flows
        }),
    )?;
    write_json_file(
        work_dir.join("bindings").join("sorx-http.template.json"),
        &handoff.sorx,
    )?;
    for (name, schema) in &handoff.schemas {
        write_json_file(
            work_dir.join("schemas").join(format!("{name}.schema.json")),
            schema,
        )?;
    }
    for flow in &handoff.flows {
        write_yaml_file(
            work_dir.join("flows").join(flow),
            &json!({"schema": "greentic.operala.flow.v1", "name": flow}),
        )?;
    }
    if handoff.capability == "reconciliation" {
        write_json_file(
            work_dir
                .join("ui")
                .join("reconciliation-exception.card.json"),
            &json!({"schema": "greentic.operala.ui.card.v1", "name": "reconciliation_exception"}),
        )?;
        write_json_file(
            work_dir.join("tests").join("one-transaction.json"),
            &json!({"transaction_id": "bank_tx_001", "amount": 1200.00, "currency": "EUR", "reference": "TEN-001"}),
        )?;
        write_json_file(
            work_dir.join("tests").join("daily-transactions.json"),
            &json!({"batch_id": "daily_001", "transactions": []}),
        )?;
        write_json_file(
            work_dir.join("tests").join("expected-decisions.json"),
            &json!([]),
        )?;
    } else if handoff.capability == "bulk_ingest" {
        write_json_file(
            work_dir.join("ui").join("bulk-upload-summary.card.json"),
            &json!({"schema": "greentic.operala.ui.card.v1", "name": "bulk_upload_summary"}),
        )?;
        write_json_file(
            work_dir.join("tests").join("bulk-upload.sample.json"),
            &json!({"batch_id": "bulk_001", "dry_run": true, "records": {}}),
        )?;
    }
    Ok(())
}

fn write_operala_gtpack(path: &Path, handoff: &OperaLaHandoff) -> OperalaResult<()> {
    if path.is_file() {
        fs::remove_file(path)
            .map_err(|err| format!("failed to replace pack file {}: {err}", path.display()))?;
    } else if path.is_dir() {
        fs::remove_dir_all(path).map_err(|err| {
            format!(
                "failed to replace pack directory {} with archive: {err}",
                path.display()
            )
        })?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let pack_name = format!(
        "{}-{}",
        handoff.sorla.package_name,
        sanitize_pack_segment(&handoff.capability)
    );
    let pack_id = format!("ai.greentic.operala.{}", sanitize_pack_segment(&pack_name));
    let entry_flow = handoff
        .flows
        .first()
        .map(|flow| flow.trim_end_matches(".flow.yaml").to_string())
        .unwrap_or_else(|| "operala-entry".to_string());
    let meta = PackMeta {
        pack_version: PACK_VERSION,
        pack_id,
        version: Version::parse(handoff.extension_version.as_str()).map_err(to_string)?,
        name: pack_name,
        kind: None,
        description: Some(format!(
            "OperaLa {} operational handoff for {}",
            handoff.capability, handoff.sorla.required_schema
        )),
        authors: vec!["Greentic OperaLa".to_string()],
        license: Some("MIT".to_string()),
        homepage: None,
        support: None,
        vendor: Some("Greentic".to_string()),
        imports: Vec::new(),
        entry_flows: vec![entry_flow.clone()],
        created_at_utc: "1970-01-01T00:00:00Z".to_string(),
        events: None,
        repo: None,
        messaging: None,
        interfaces: Vec::new(),
        annotations: serde_json::Map::from_iter([
            ("greentic.operala.schema".to_string(), json!(HANDOFF_SCHEMA)),
            (
                "greentic.operala.capability".to_string(),
                json!(handoff.capability),
            ),
            (
                "greentic.operala.extension".to_string(),
                json!(handoff.extension),
            ),
            (
                "greentic.sorla.source_digest".to_string(),
                json!(handoff.sorla.source_digest),
            ),
        ]),
        distribution: None,
        components: Vec::new(),
    };

    let mut builder = PackBuilder::new(meta)
        .with_signing(Signing::None)
        .with_provenance(Provenance {
            builder: format!("greentic-operala@{}", env!("CARGO_PKG_VERSION")),
            git_commit: None,
            git_repo: Some("https://github.com/greenticai/greentic-operala".to_string()),
            toolchain: None,
            built_at_utc: "1970-01-01T00:00:00Z".to_string(),
            host: None,
            notes: Some("OperaLa handoff pack built with greentic-pack".to_string()),
        })
        .with_asset_bytes(
            "operala/operala-handoff.json",
            serde_json::to_vec_pretty(handoff).map_err(to_string)?,
        )
        .with_asset_bytes(
            "operala/operala.yaml",
            serde_yaml::to_string(handoff)
                .map_err(to_string)?
                .into_bytes(),
        );

    for flow in &handoff.flows {
        let flow_id = flow.trim_end_matches(".flow.yaml");
        let flow_yaml = format!(
            "id: {flow_id}\ntype: messaging\nschema_version: 2\nstart: operala_handoff\nnodes:\n  operala_handoff:\n    op: {{}}\n    routing: out\n"
        );
        let flow_json = json!({
            "id": flow_id,
            "type": "messaging",
            "schema_version": 2,
            "start": "operala_handoff",
            "nodes": {
                "operala_handoff": {
                    "op": {},
                    "routing": "out"
                }
            }
        });
        let flow_bundle = FlowBundle {
            id: flow_id.to_string(),
            kind: "messaging".to_string(),
            entry: "operala_handoff".to_string(),
            yaml: flow_yaml.clone(),
            json: flow_json.clone(),
            hash_blake3: blake3::hash(&serde_json::to_vec(&flow_json).map_err(to_string)?)
                .to_hex()
                .to_string(),
            nodes: Vec::new(),
        };
        builder = builder
            .with_flow(flow_bundle)
            .with_asset_bytes(format!("operala/flows/{flow}"), flow_yaml.into_bytes());
    }
    for (name, schema) in &handoff.schemas {
        builder = builder.with_asset_bytes(
            format!("operala/schemas/{name}.schema.json"),
            serde_json::to_vec_pretty(schema).map_err(to_string)?,
        );
    }
    builder.build(path).map_err(to_string)?;
    Ok(())
}

fn sanitize_pack_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "operala".to_string()
    } else {
        sanitized
    }
}

fn bank_transaction_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://greentic.ai/schemas/operala.reconciliation.bank-transaction.v1.json",
        "type": "object",
        "required": ["transaction_id", "booked_at", "amount", "currency", "reference"],
        "properties": {
            "transaction_id": {"type": "string"},
            "booked_at": {"type": "string"},
            "amount": {"type": "number"},
            "currency": {"type": "string"},
            "reference": {"type": "string"}
        }
    })
}

fn daily_bank_transactions_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://greentic.ai/schemas/operala.reconciliation.daily-bank-transactions.v1.json",
        "type": "object",
        "required": ["batch_id", "transactions"],
        "properties": {
            "batch_id": {"type": "string"},
            "transactions": {"type": "array", "items": bank_transaction_schema()}
        }
    })
}

fn yaml_named_list(yaml: &serde_yaml::Value, key: &str) -> Vec<String> {
    yaml.get(key)
        .and_then(serde_yaml::Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("name")
                .and_then(serde_yaml::Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn yaml_id_list(yaml: &serde_yaml::Value, key: &str) -> Vec<String> {
    yaml.get(key)
        .and_then(serde_yaml::Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("id")
                .and_then(serde_yaml::Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn yaml_string(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_str)
        .map(ToString::to_string)
}

fn pick(candidates: &[String], preferred: &[&str]) -> Option<String> {
    preferred
        .iter()
        .find(|name| candidates.iter().any(|candidate| candidate == **name))
        .map(|value| (*value).to_string())
        .or_else(|| candidates.first().cloned())
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    let mut previous_was_separator = false;
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 && !previous_was_separator {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator && !output.is_empty() {
            output.push('_');
            previous_was_separator = true;
        }
    }
    output.trim_matches('_').to_string()
}

fn map<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn default_true() -> bool {
    true
}

fn resolve_local_path(
    reference: &str,
    tenant: Option<&str>,
    team: Option<&str>,
) -> OperalaResult<PathBuf> {
    let resolver = LocalCacheArtifactResolver::from_env();
    match resolver.resolve_sync(reference, tenant, team)? {
        ResolvedArtifact::LocalPath(path) => Ok(path),
        ResolvedArtifact::Bytes(_) | ResolvedArtifact::Json(_) | ResolvedArtifact::Yaml(_) => {
            Err("expected artifact resolver to return a path".to_string())
        }
    }
}

fn write_json_file<T: Serialize, P: AsRef<Path>>(path: P, value: &T) -> OperalaResult<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(to_string)?;
    fs::write(path, bytes).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn write_yaml_file<T: Serialize, P: AsRef<Path>>(path: P, value: &T) -> OperalaResult<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let text = serde_yaml::to_string(value).map_err(to_string)?;
    fs::write(path, text).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn to_string<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

fn has_help(args: &[OsString]) -> bool {
    args.iter().skip(1).any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--help" || arg == "-h"
    })
}

fn explicit_locale_arg(args: &[OsString]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let arg = arg.to_string_lossy();
        if arg == "--locale" {
            return iter.next().map(|value| value.to_string_lossy().to_string());
        }
        if let Some(value) = arg.strip_prefix("--locale=") {
            return Some(value.to_string());
        }
    }
    None
}

fn locale_from_args(args: &[OsString]) -> Option<String> {
    explicit_locale_arg(args)
        .or_else(|| env::var("OPERALA_LOCALE").ok())
        .or_else(|| env::var("LANG").ok())
}

fn normalized_locale(locale: Option<&str>) -> String {
    let requested = locale
        .unwrap_or("en-GB")
        .trim()
        .split('.')
        .next()
        .unwrap_or("en-GB")
        .replace('_', "-");
    if requested.is_empty() {
        return "en-GB".to_string();
    }
    if included_locale_json(&requested).is_some() {
        return requested;
    }
    if let Some((language, _)) = requested.split_once('-')
        && included_locale_json(language).is_some()
    {
        return language.to_string();
    }
    "en-GB".to_string()
}

fn t(key: &str, locale: Option<&str>) -> String {
    let locale = normalized_locale(locale);
    catalog(&locale)
        .get(key)
        .cloned()
        .or_else(|| catalog("en-GB").get(key).cloned())
        .unwrap_or_else(|| key.to_string())
}

fn catalog(locale: &str) -> BTreeMap<String, String> {
    included_locale_json(locale)
        .or_else(|| included_locale_json("en-GB"))
        .or_else(|| included_locale_json("en"))
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

fn included_locale_json(locale: &str) -> Option<&'static str> {
    embedded_i18n::locale_json(locale)
}

pub fn supported_locales() -> &'static [&'static str] {
    embedded_i18n::supported_locales()
}

fn text_direction(locale: &str) -> &'static str {
    if locale.starts_with("ar") {
        "rtl"
    } else {
        "ltr"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperalaHelpCommand {
    Root,
    Prompt,
    Wizard,
}

fn localized_operala_help_for_args(args: &[OsString]) -> Option<String> {
    if !has_help(args) {
        return None;
    }
    let locale = locale_from_args(args);
    Some(match operala_help_command(args) {
        OperalaHelpCommand::Root => localized_operala_help(locale.as_deref()),
        OperalaHelpCommand::Prompt => localized_operala_prompt_help(locale.as_deref()),
        OperalaHelpCommand::Wizard => localized_operala_wizard_help(locale.as_deref()),
    })
}

fn operala_help_command(args: &[OsString]) -> OperalaHelpCommand {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let arg = arg.to_string_lossy();
        match arg.as_ref() {
            "prompt" => return OperalaHelpCommand::Prompt,
            "wizard" => return OperalaHelpCommand::Wizard,
            "--help" | "-h" => return OperalaHelpCommand::Root,
            "--locale" => {
                let _ = iter.next();
            }
            value if value.starts_with("--locale=") => {}
            value if value.starts_with('-') => {}
            _ => return OperalaHelpCommand::Root,
        }
    }
    OperalaHelpCommand::Root
}

fn localized_operala_help(locale: Option<&str>) -> String {
    format!(
        "{about}\n\n{usage}: greentic-operala <COMMAND>\n\n{commands}:\n  prompt    {prompt}\n  wizard    {wizard}\n\n{options}:\n      --locale <LOCALE>  {locale_option}\n  -h, --help             {help_option}\n",
        about = t("operala.cli.about", locale),
        usage = t("operala.cli.usage", locale),
        commands = t("operala.cli.commands", locale),
        prompt = t("operala.cli.prompt.about", locale),
        wizard = t("operala.cli.wizard.about", locale),
        options = t("operala.cli.options", locale),
        locale_option = t("operala.cli.option.locale", locale),
        help_option = t("operala.cli.option.help", locale)
    )
}

fn localized_operala_prompt_help(locale: Option<&str>) -> String {
    format!(
        "{about}\n\n{usage}: greentic-operala prompt --sorla <FILE> [OPTIONS] <PROMPT>\n\n{options}:\n      --sorla <FILE>     {sorla_option}\n      --locale <LOCALE>  {locale_option}\n      --output <FILE>    {output_option}\n      --tenant <TENANT>  {tenant_option}\n      --team <TEAM>      {team_option}\n  -h, --help             {help_option}\n",
        about = t("operala.cli.prompt.about", locale),
        usage = t("operala.cli.usage", locale),
        options = t("operala.cli.options", locale),
        sorla_option = t("operala.cli.option.sorla", locale),
        locale_option = t("operala.cli.option.locale", locale),
        output_option = t("operala.cli.option.output", locale),
        tenant_option = t("operala.cli.option.tenant", locale),
        team_option = t("operala.cli.option.team", locale),
        help_option = t("operala.cli.option.help", locale)
    )
}

fn localized_operala_wizard_help(locale: Option<&str>) -> String {
    format!(
        "{about}\n\n{usage}: greentic-operala wizard [OPTIONS]\n\n{options}:\n      --schema           {schema_option}\n      --answers <REF>    {answers_option}\n      --locale <LOCALE>  {locale_option}\n  -h, --help             {help_option}\n",
        about = t("operala.cli.wizard.about", locale),
        usage = t("operala.cli.usage", locale),
        options = t("operala.cli.options", locale),
        schema_option = t("operala.cli.option.schema", locale),
        answers_option = t("operala.cli.option.answers", locale),
        locale_option = t("operala.cli.option.locale", locale),
        help_option = t("operala.cli.option.help", locale)
    )
}

fn distributed_cache_file_name(reference: &str) -> String {
    let escaped = reference
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{escaped}.json")
}

fn greentic_qa_engine() -> &'static str {
    let _ = std::any::type_name::<greentic_qa_lib::WizardRunConfig>();
    "greentic-qa-lib"
}

fn is_distributed_reference(reference: &str) -> bool {
    ["oci://", "store://", "repo://"]
        .iter()
        .any(|scheme| reference.starts_with(scheme))
}

fn verify_reference_digest(
    reference: &str,
    declared_digest: Option<&str>,
    actual_digest: &str,
) -> OperalaResult<()> {
    let uri_digest = reference
        .split_once("@sha256:")
        .map(|(_, digest)| format!("sha256:{digest}"));
    for expected in declared_digest.into_iter().chain(uri_digest.as_deref()) {
        if expected != actual_digest {
            return Err(format!(
                "digest mismatch for `{reference}`: expected {expected}, got {actual_digest}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_fixture_answers() {
        let answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        validate_answers(&answers).expect("fixture answers validate");
    }

    #[test]
    fn fixture_sorla_contract_has_reconciliation_parts() {
        let sorla = load_sorla_contract(&SourceRef {
            kind: SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            digest: None,
        })
        .expect("fixture sorla loads");
        assert!(sorla.records.iter().any(|name| name == "RentObligation"));
        assert!(sorla.events.iter().any(|name| name == "BankTransaction"));
        assert!(sorla.actions.iter().any(|name| name == "create_payment"));
        assert!(
            sorla
                .agent_endpoints
                .iter()
                .any(|name| name == "create_payment")
        );
    }

    #[test]
    fn distributed_reference_reports_cache_location() {
        let err = resolve_local_path(
            "store://customer-store/demo/operala/answers/rent-recon",
            Some("demo"),
            None,
        )
        .expect_err("cache should be missing");
        assert!(err.contains("was accepted"));
        assert!(err.contains("greentic-distributor-client"));
    }

    #[test]
    fn local_cache_resolver_resolves_distributed_reference() {
        let root =
            std::env::temp_dir().join(format!("operala-resolver-test-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create temp root");
        let reference = "oci://ghcr.io/greenticai/customer/answers/rent-recon:1.0.0";
        let path = root.join(distributed_cache_file_name(reference));
        fs::write(&path, b"{}").expect("write cached artifact");
        let resolver = LocalCacheArtifactResolver::with_root(root.clone());
        let resolved = resolver
            .resolve_sync(reference, Some("demo-tenant"), Some("property-ops"))
            .expect("distributed ref resolves from local cache");
        match resolved {
            ResolvedArtifact::LocalPath(actual) => assert_eq!(actual, path),
            _ => panic!("expected local path"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn localized_help_uses_requested_locale() {
        let help = localized_operala_help(Some("nl-NL"));
        assert!(help.contains("Gebruik"));
        assert!(help.contains("Commando"));
    }

    #[test]
    fn localized_help_accepts_locale_after_help_flag() {
        let root_help = localized_operala_help_for_args(&[
            OsString::from("greentic-operala"),
            OsString::from("--help"),
            OsString::from("--locale"),
            OsString::from("de"),
        ])
        .expect("root help should localize");
        assert!(root_help.contains("Verwendung"));
        assert!(root_help.contains("Befehle"));

        let prompt_help = localized_operala_help_for_args(&[
            OsString::from("greentic-operala"),
            OsString::from("prompt"),
            OsString::from("--help"),
            OsString::from("--locale=de"),
        ])
        .expect("prompt help should localize");
        assert!(prompt_help.contains("SoRLa-YAML-Quelldatei"));
    }

    #[test]
    fn locale_normalization_accepts_supported_language_tags() {
        assert_eq!(normalized_locale(Some("es")), "es");
        assert_eq!(normalized_locale(Some("es-MX")), "es");
        assert_eq!(normalized_locale(Some("pt_BR.UTF-8")), "pt");
        assert_eq!(normalized_locale(Some("zz-ZZ")), "en-GB");
    }

    #[test]
    fn bundled_i18n_catalogs_cover_supported_locales() {
        let raw_locales = included_locale_json("locales").expect("locales list is bundled");
        let locales: Vec<String> =
            serde_json::from_str(raw_locales).expect("locales list should parse");
        let embedded_locales = supported_locales();
        assert!(locales.len() > 60, "SoRLa locale set should be mirrored");
        assert_eq!(
            embedded_locales,
            locales.iter().map(String::as_str).collect::<Vec<_>>(),
            "compiled supported locale list should match i18n/locales.json"
        );
        for expected in ["ar-AE", "es", "pt", "zh"] {
            assert!(
                locales.iter().any(|locale| locale == expected),
                "locale `{expected}` should be supported"
            );
        }

        for locale in locales {
            let raw = included_locale_json(&locale)
                .unwrap_or_else(|| panic!("locale `{locale}` should be bundled"));
            serde_json::from_str::<BTreeMap<String, String>>(raw)
                .unwrap_or_else(|err| panic!("bundled locale `{locale}` should parse: {err}"));
        }
    }

    #[test]
    fn validation_rejects_missing_required_answer_fields() {
        let base: Value = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture json parses");
        for field in ["schema", "intent", "sorla", "extension", "outputs"] {
            let mut value = base.clone();
            value.as_object_mut().unwrap().remove(field);
            let parsed = serde_json::from_value::<OperalaAnswers>(value);
            assert!(parsed.is_err(), "missing {field} should fail to parse");
        }

        let mut no_tenant = serde_json::from_value::<OperalaAnswers>(base).unwrap();
        no_tenant.tenant = None;
        let err = validate_answers(&no_tenant).expect_err("tenant is required");
        assert!(err.contains("tenant"));
    }

    #[test]
    fn locale_and_team_are_optional() {
        let mut answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        answers.locale = None;
        answers.team = None;
        validate_answers(&answers).expect("locale and team are optional");
    }

    #[test]
    fn validation_rejects_incomplete_reconciliation_answers() {
        let answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");

        let mut missing_currency = answers.clone();
        missing_currency
            .capability_answers
            .reconciliation
            .as_mut()
            .unwrap()
            .source_fields
            .remove("currency");
        let err = validate_answers(&missing_currency).expect_err("currency mapping is required");
        assert!(err.contains("source_fields.currency"));

        let mut invalid_mode = answers.clone();
        invalid_mode
            .capability_answers
            .reconciliation
            .as_mut()
            .unwrap()
            .input_modes = vec!["stream".to_string()];
        let err = validate_answers(&invalid_mode).expect_err("mode should be restricted");
        assert!(err.contains("unsupported mode"));

        let mut invalid_threshold = answers;
        invalid_threshold
            .capability_answers
            .reconciliation
            .as_mut()
            .unwrap()
            .matching
            .auto_match_threshold = 101;
        let err = validate_answers(&invalid_threshold).expect_err("threshold should be bounded");
        assert!(err.contains("0 to 100"));
    }

    #[test]
    fn prompt_infers_reconciliation_answers_from_payment_language() {
        let answers = prompt_answers(&PromptArgs {
            sorla: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            tenant: Some("acme-property".to_string()),
            team: Some("property-ops".to_string()),
            locale: Some("en-GB".to_string()),
            output: None,
            llm_provider: None,
            llm_model: None,
            no_llm: false,
            existing: None,
            in_place: false,
            prompt: "Set up rent payment reconciliation from bank transactions".to_string(),
        })
        .expect("prompt produces answers");

        assert_eq!(answers.schema, ANSWERS_SCHEMA);
        assert_eq!(answers.extension, EXTENSION_RECONCILIATION);
        assert_eq!(
            answers.detected_capability.as_deref(),
            Some("reconciliation")
        );
        let reconciliation = answers
            .capability_answers
            .reconciliation
            .as_ref()
            .expect("reconciliation answers are nested");
        assert_eq!(reconciliation.source_event, "BankTransaction");
        assert_eq!(
            reconciliation.actions.get("create_settlement").unwrap(),
            "create_payment"
        );
        assert_eq!(
            reconciliation
                .agent_endpoints
                .get("create_settlement")
                .unwrap(),
            "create_payment"
        );
        validate_answers(&answers).expect("prompt answers validate");
    }

    #[test]
    fn prompt_reports_follow_up_when_capability_is_unclear() {
        let err = prompt_answers(&PromptArgs {
            sorla: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            tenant: Some("acme-property".to_string()),
            team: None,
            locale: None,
            output: None,
            llm_provider: None,
            llm_model: None,
            no_llm: false,
            existing: None,
            in_place: false,
            prompt: "Help my operations team with something".to_string(),
        })
        .expect_err("unclear prompt should need follow-up");

        assert!(err.contains("follow-up required"));
    }

    #[test]
    fn wizard_schema_exposes_reconciliation_questions() {
        let schema = wizard_schema(Some("en-GB"));
        let extensions = schema["extensions"]
            .as_array()
            .expect("extensions are present");
        let reconciliation = extensions
            .iter()
            .find(|extension| extension["flow"] == "operala.reconciliation")
            .expect("reconciliation extension schema is registered");
        let source_questions = reconciliation["sections"][0]["questions"]
            .as_array()
            .expect("source questions are present");
        assert!(
            source_questions
                .iter()
                .any(|question| question["id"] == "source_event")
        );
        assert!(
            source_questions
                .iter()
                .any(|question| question["id"] == "expected_record")
        );
        assert!(
            extensions
                .iter()
                .any(|extension| extension["flow"] == "operala.bulk_ingest")
        );
    }

    #[test]
    fn prompt_infers_generic_bulk_ingest_answers() {
        let answers = prompt_answers(&PromptArgs {
            sorla: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            tenant: Some("demo-tenant".to_string()),
            team: Some("finance".to_string()),
            locale: Some("en-GB".to_string()),
            output: None,
            llm_provider: None,
            llm_model: None,
            no_llm: false,
            existing: None,
            in_place: false,
            prompt: "Create a generic bulk upload operation from a JSON batch file with exactly 3 tenants, 3 tenancies, and 6 payments.".to_string(),
        })
        .expect("bulk upload prompt produces answers");

        assert_eq!(answers.extension, EXTENSION_BULK_INGEST);
        assert_eq!(answers.detected_capability.as_deref(), Some("bulk_ingest"));
        let bulk = answers
            .capability_answers
            .bulk_ingest
            .as_ref()
            .expect("bulk ingest answers are nested");
        assert_eq!(bulk.name, "generic_bulk_ingest");
        assert_eq!(bulk.record_collections.get("tenant").unwrap(), "Tenant");
        assert_eq!(bulk.record_collections.get("payment").unwrap(), "Payment");
        assert_eq!(bulk.expected_counts.get("Tenant"), Some(&3));
        assert_eq!(bulk.expected_counts.get("Tenancy"), Some(&3));
        assert_eq!(bulk.expected_counts.get("Payment"), Some(&6));
        validate_answers(&answers).expect("bulk ingest answers validate");
    }

    #[test]
    fn readiness_reports_ready_missing_and_ambiguous_states() {
        let sorla = load_sorla_contract(&SourceRef {
            kind: SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            digest: None,
        })
        .expect("fixture sorla loads");
        let mut answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        answers.locale = Some("nl-NL".to_string());

        let ready = RECONCILIATION_EXTENSION
            .analyse_sorla(&sorla, &answers)
            .expect("ready analysis succeeds");
        assert_eq!(ready.status, ReadinessStatus::Ready);
        assert_eq!(
            ready.found.get("source_event").unwrap(),
            &Value::String("BankTransaction".to_string())
        );
        assert!(ready.summary.contains("kan worden gegenereerd"));

        let mut missing_answers = answers.clone();
        missing_answers
            .capability_answers
            .reconciliation
            .as_mut()
            .unwrap()
            .expected_record = "MissingInvoice".to_string();
        let missing = RECONCILIATION_EXTENSION
            .analyse_sorla(&sorla, &missing_answers)
            .expect("missing analysis succeeds");
        assert_eq!(missing.status, ReadinessStatus::NeedsSorlaChanges);
        assert!(
            missing
                .missing
                .iter()
                .any(|item| item.contains("MissingInvoice"))
        );

        let mut ambiguous_sorla = sorla.clone();
        ambiguous_sorla.records.push("Invoice".to_string());
        let mut ambiguous_answers = answers;
        ambiguous_answers
            .capability_answers
            .reconciliation
            .as_mut()
            .unwrap()
            .expected_record
            .clear();
        let ambiguous = RECONCILIATION_EXTENSION
            .analyse_sorla(&ambiguous_sorla, &ambiguous_answers)
            .expect("ambiguous analysis succeeds");
        assert_eq!(ambiguous.status, ReadinessStatus::UnsafeOrAmbiguous);
        assert!(
            ambiguous
                .warnings
                .iter()
                .any(|item| item.contains("expected_record is ambiguous"))
        );
    }

    #[test]
    fn missing_records_generate_additive_sorla_patch_proposal() {
        let sorla = load_sorla_contract(&SourceRef {
            kind: SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".to_string(),
            digest: None,
        })
        .expect("fixture sorla loads");
        let mut answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        let root = std::env::temp_dir().join(format!(
            "operala-patch-proposal-test-{}",
            std::process::id()
        ));
        answers.outputs.work_dir = root.join("work");
        answers.outputs.handoff_path = Some(root.join("work").join("operala-handoff.json"));
        answers.outputs.gtpack_path = None;
        let recon = answers.capability_answers.reconciliation.as_mut().unwrap();
        recon.settlement_record = "MissingPayment".to_string();
        recon.exception_record = "MissingReconciliationCase".to_string();

        let readiness = RECONCILIATION_EXTENSION
            .analyse_sorla(&sorla, &answers)
            .expect("readiness analysis succeeds");
        let patch = sorla_patch_proposal(&readiness, &sorla).expect("patch proposal validates");
        assert_eq!(patch["schema"], "greentic.sorla.patch.v1");
        assert_eq!(patch["source"]["kind"], "sorla-yaml");
        assert_eq!(patch["status"], "proposed");
        assert_eq!(patch["operations"][0]["op"], "add_record");
        assert_eq!(patch["operations"][0]["record"]["name"], "missing_payment");
        assert_eq!(
            patch["operations"][0]["record"]["requested_name"],
            "MissingPayment"
        );
        assert_eq!(
            patch["operations"][1]["record"]["name"],
            "missing_reconciliation_case"
        );

        run_wizard(&answers).expect("wizard writes patch proposal");
        let written: Value =
            serde_json::from_slice(&fs::read(root.join("work").join("sorla.patch.json")).unwrap())
                .expect("written patch parses");
        assert_eq!(written["status"], "proposed");
        let summary =
            fs::read_to_string(root.join("work").join("build.summary.md")).expect("summary exists");
        assert!(summary.contains("SoRLa patch proposal: proposed, not applied"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wizard_writes_resumable_state_and_summary() {
        let mut answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        let root =
            std::env::temp_dir().join(format!("operala-wizard-state-test-{}", std::process::id()));
        answers.outputs.work_dir = root.join("work");
        answers.outputs.handoff_path = Some(root.join("work").join("operala-handoff.json"));
        answers.outputs.gtpack_path = Some(root.join("pack.gtpack"));

        let first = run_wizard(&answers).expect("first wizard run succeeds");
        assert_eq!(first["status"], "ready");
        let state_path = root.join("work").join("operala.state.json");
        let state: WizardState =
            serde_json::from_slice(&fs::read(&state_path).expect("state exists"))
                .expect("state parses");
        assert_eq!(state.status, "complete");
        assert!(!state.resumed_from_existing_state);
        assert!(state.stages.iter().all(|stage| stage.status == "complete"));

        let summary =
            fs::read_to_string(root.join("work").join("build.summary.md")).expect("summary exists");
        assert!(summary.contains("Unresolved questions: none"));
        let handoff: OperaLaHandoff = serde_json::from_slice(
            &fs::read(root.join("work").join("operala-handoff.json")).unwrap(),
        )
        .expect("handoff parses");
        assert_eq!(handoff.bindings["source_event"], "BankTransaction");
        assert_eq!(
            handoff.bindings["actions"]["create_settlement"],
            "create_payment"
        );
        assert_eq!(
            handoff.bindings["agent_endpoints"]["create_settlement"],
            "create_payment"
        );
        assert_eq!(
            handoff.bindings["source_digest"],
            Value::String(handoff.sorla.source_digest.clone())
        );
        let pack_path = root.join("pack.gtpack");
        assert!(pack_path.is_file());
        let work_dir_pack_path = root.join("work").join("tenancy-rent-reconciliation.gtpack");
        assert!(work_dir_pack_path.is_file());
        assert_eq!(
            first["gtpack_path"],
            Value::String(work_dir_pack_path.to_string_lossy().to_string())
        );
        assert_eq!(
            first["configured_gtpack_path"],
            Value::String(pack_path.to_string_lossy().to_string())
        );
        let file = fs::File::open(&pack_path).expect("pack opens");
        let mut archive = zip::ZipArchive::new(file).expect("pack is a zip archive");
        assert!(archive.by_name("manifest.cbor").is_ok());
        assert!(archive.by_name("sbom.json").is_ok());
        assert!(
            archive
                .by_name("flows/ingest-transaction/flow.ygtc")
                .is_ok()
        );
        assert!(
            archive
                .by_name("assets/operala/operala-handoff.json")
                .is_ok()
        );
        assert!(
            archive
                .by_name("assets/operala/flows/ingest-transaction.flow.yaml")
                .is_ok()
        );
        assert!(
            archive
                .by_name("assets/operala/schemas/bank-transaction.schema.json")
                .is_ok()
        );

        let second = run_wizard(&answers).expect("second wizard run resumes");
        assert_eq!(second["status"], "ready");
        let state: WizardState =
            serde_json::from_slice(&fs::read(&state_path).expect("state exists"))
                .expect("state parses");
        assert!(state.resumed_from_existing_state);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_finds_reconciliation_and_localizes_unknown_extension() {
        let registry = ExtensionRegistry::built_in();
        assert!(registry.get(EXTENSION_RECONCILIATION).is_some());

        let mut answers: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        let root = std::env::temp_dir().join(format!(
            "operala-unknown-extension-test-{}",
            std::process::id()
        ));
        answers.outputs.work_dir = root;
        answers.extension = "greentic.operala.unknown.v1".to_string();
        answers.locale = Some("nl-NL".to_string());
        let err = run_wizard(&answers).expect_err("unknown extension should fail");
        assert!(err.contains("Onbekende OperaLa-extensie"));
    }

    #[test]
    fn prompt_args_parse_llm_flags() {
        use clap::Parser;
        let cli = OperalaCli::parse_from([
            "greentic-operala",
            "prompt",
            "--sorla",
            "s.yaml",
            "--llm-provider",
            "anthropic",
            "--llm-model",
            "claude-sonnet-4-6",
            "--existing",
            "old-answers.json",
            "--in-place",
            "update the tolerance",
        ]);
        let OperalaCommand::Prompt(args) = cli.command else {
            panic!("expected prompt command");
        };
        assert_eq!(
            args.llm_provider,
            Some(greentic_llm::ProviderKind::Anthropic)
        );
        assert_eq!(args.llm_model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(
            args.existing.as_deref(),
            Some(std::path::Path::new("old-answers.json"))
        );
        assert!(args.in_place);
        assert!(!args.no_llm);
    }

    #[test]
    fn reconciliation_answers_schema_round_trips_the_fixture() {
        let schema = RECONCILIATION_EXTENSION.answers_schema();
        assert_eq!(schema["type"], "object");
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for field in [
            "name",
            "source_event",
            "expected_record",
            "settlement_record",
            "exception_record",
            "input_modes",
            "source_fields",
            "expected_fields",
            "matching",
            "exception_policy",
            "actions",
            "agent_endpoints",
        ] {
            assert!(required.contains(&field), "missing required field {field}");
        }
    }

    #[test]
    fn bulk_ingest_answers_schema_has_required_fields() {
        let schema = BULK_INGEST_EXTENSION.answers_schema();
        assert_eq!(schema["type"], "object");
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for field in [
            "name",
            "input_modes",
            "record_collections",
            "actions",
            "validation",
        ] {
            assert!(required.contains(&field), "missing required field {field}");
        }
    }

    #[test]
    fn llm_backed_prompt_produces_validated_answers() {
        let answers_value: serde_json::Value = {
            let fixture: OperalaAnswers = serde_json::from_str(include_str!(
                "../extensions/reconciliation/examples/tenancy/answers.json"
            ))
            .unwrap();
            serde_json::to_value(fixture.capability_answers.reconciliation.unwrap()).unwrap()
        };
        let chat = inference::tests_support::scripted_chat(vec![inference::tests_support::emit(
            answers_value,
        )]);
        let answers = prompt_answers_with_llm(
            &PromptArgs {
                sorla: "extensions/reconciliation/examples/tenancy/sorla.yaml".into(),
                locale: Some("en-GB".into()),
                output: None,
                tenant: Some("acme-property".into()),
                team: None,
                llm_provider: None,
                llm_model: None,
                no_llm: false,
                existing: None,
                in_place: false,
                prompt: "Set up rent payment reconciliation from bank transactions".into(),
            },
            Some(&chat),
        )
        .expect("llm prompt produces answers");
        assert_eq!(answers.extension, EXTENSION_RECONCILIATION);
        let reconciliation = answers
            .capability_answers
            .reconciliation
            .as_ref()
            .expect("nested");
        assert_eq!(reconciliation.source_event, "BankTransaction");
        validate_answers(&answers).expect("llm answers validate");
    }

    #[test]
    fn no_llm_path_is_byte_identical_to_legacy_keyword_path() {
        let args = PromptArgs {
            sorla: "extensions/reconciliation/examples/tenancy/sorla.yaml".into(),
            locale: Some("en-GB".into()),
            output: None,
            tenant: Some("acme-property".into()),
            team: Some("property-ops".into()),
            llm_provider: None,
            llm_model: None,
            no_llm: true,
            existing: None,
            in_place: false,
            prompt: "Set up rent payment reconciliation from bank transactions".into(),
        };
        let via_wrapper = prompt_answers(&args).expect("wrapper works");
        let via_llm_none = prompt_answers_with_llm(&args, None).expect("explicit none works");
        assert_eq!(
            serde_json::to_string(&via_wrapper).unwrap(),
            serde_json::to_string(&via_llm_none).unwrap()
        );
    }

    #[test]
    fn update_mode_changes_only_the_instructed_field() {
        let existing: OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .unwrap();
        let mut updated_value =
            serde_json::to_value(existing.capability_answers.reconciliation.clone().unwrap())
                .unwrap();
        updated_value["matching"]["amount_tolerance"] = serde_json::json!(5.0);
        let chat = inference::tests_support::scripted_chat(vec![inference::tests_support::emit(
            updated_value,
        )]);

        let outcome = inference::update_answers(
            &chat,
            &existing,
            "extensions/reconciliation/examples/tenancy/sorla.yaml",
            "raise the amount tolerance to 5",
        )
        .expect("update succeeds");

        let updated_reconciliation = outcome
            .answers
            .capability_answers
            .reconciliation
            .as_ref()
            .expect("nested");
        assert_eq!(updated_reconciliation.matching.amount_tolerance, 5.0);
        // Envelope preserved:
        assert_eq!(outcome.answers.tenant, existing.tenant);
        assert_eq!(outcome.answers.outputs.work_dir, existing.outputs.work_dir);
        // Diff names the changed capability path (intent also changes — assert the capability change is present):
        assert!(
            outcome
                .diff
                .iter()
                .any(|e| e.path == "capability_answers.reconciliation.matching.amount_tolerance"),
            "diff: {:?}",
            outcome.diff
        );
    }
}
