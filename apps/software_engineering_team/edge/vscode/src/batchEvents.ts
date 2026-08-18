export type BatchEventType =
  | 'BATCH_AGENT_REGISTERED'
  | 'BATCH_AGENT_STARTED'
  | 'BATCH_AGENT_STOPPED'
  | 'BATCH_AGENT_ERROR'
  | 'BATCH_EXTRACTION_STARTED'
  | 'BATCH_EXTRACTION_COMPLETED'
  | 'BATCH_EXTRACTION_FAILED'
  | 'BATCH_ENTITY_DETECTED'
  | 'BATCH_RELATION_DETECTED'
  | 'BATCH_INTENT_DETECTED'
  | 'BATCH_DECISION_DETECTED'
  | 'BATCH_CONTEXT_INJECTED';

export interface BatchEventEnvelope {
  channel: 'batch';
  event_type: BatchEventType;
  source: string;
  task_iri: string;
  timestamp: string;
  payload: Record<string, unknown>;
}

export interface BatchExtractionPayload {
  agent_name: string;
  source_window: unknown[];
  extractor: string;
  duration_ms?: number;
  matched_entities?: number;
  matched_relations?: number;
  error?: string;
}

export interface BatchEntityPayload {
  entity_type: string;
  entity_iri: string;
  confidence: number;
  properties: Record<string, unknown>;
}

export interface BatchRelationPayload {
  relation_type: string;
  relation_iri: string;
  source_iri: string;
  target_iri: string;
  confidence: number;
}

export interface BatchIntentPayload {
  intent_type: string;
  intent_iri: string;
  confidence: number;
}

export interface BatchDecisionPayload {
  prompt_source: string;
  decision_type: string;
  confidence: number;
  criteria_used: string[];
}

export interface BatchAgentErrorPayload {
  agent_name: string;
  error: string;
  error_type: string;
  recoverable: boolean;
}

export interface BatchContextInjectedPayload {
  agent_name: string;
  context_sources: string[];
  total_triples: number;
  l0_items: number;
  l3_projection: string;
  kg_entities: string[];
}

export function isBatchEvent(data: unknown): data is BatchEventEnvelope {
  return (
    typeof data === 'object' &&
    data !== null &&
    (data as Record<string, unknown>).channel === 'batch'
  );
}

export function parseBatchEvent(data: string): BatchEventEnvelope | null {
  try {
    const parsed = JSON.parse(data);
    if (isBatchEvent(parsed)) {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}
