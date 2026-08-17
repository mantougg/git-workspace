import { invoke } from "@tauri-apps/api/core";
import type {
  DagGraph,
  DagSubmitRequest,
  FailurePolicy,
  Pipeline,
  PipelineRunReport,
  RepoSelection,
} from "@/types/pipeline";

// ---------------------------------------------------------------------------
// T-24 Task DAG
// ---------------------------------------------------------------------------

/** Submit a dependency DAG of tasks; returns the DAG id. */
export function submitDagTasks(request: DagSubmitRequest): Promise<string> {
  return invoke<string>("submit_dag_tasks", { request });
}

/** DAG visualization payload (nodes + edges + live states). */
export function getDagGraph(dagId: string): Promise<DagGraph> {
  return invoke<DagGraph>("get_dag_graph", { dagId });
}

/** Cancel a whole DAG run (propagates along dependency edges). */
export function cancelDag(dagId: string): Promise<void> {
  return invoke<void>("cancel_dag", { dagId });
}

// ---------------------------------------------------------------------------
// T-23 Workspace Pipeline
// ---------------------------------------------------------------------------

export function listPipelineTemplates(): Promise<Pipeline[]> {
  return invoke<Pipeline[]>("list_pipeline_templates");
}

/** Save (upsert) a template; returns the saved template (id assigned). */
export function savePipelineTemplate(template: Pipeline): Promise<Pipeline> {
  return invoke<Pipeline>("save_pipeline_template", { template });
}

export function deletePipelineTemplate(templateId: string): Promise<void> {
  return invoke<void>("delete_pipeline_template", { templateId });
}

/** Built-in sample flow (unsaved). */
export function getSamplePipeline(): Promise<Pipeline> {
  return invoke<Pipeline>("get_sample_pipeline");
}

/** Compile + submit a pipeline run; returns the run id (= DAG id). */
export function runPipeline(
  pipeline: Pipeline,
  repos: RepoSelection[],
  onFailure?: FailurePolicy,
): Promise<string> {
  return invoke<string>("run_pipeline", {
    pipeline,
    repos,
    onFailure: onFailure ?? null,
  });
}

/** Execution report of a run (per-step + per-repo, durations, partials). */
export function getPipelineRun(
  runId: string,
  pipeline: Pipeline,
): Promise<PipelineRunReport> {
  return invoke<PipelineRunReport>("get_pipeline_run", { runId, pipeline });
}
