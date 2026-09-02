import type { MainProjection, UiError } from "./bridge";

export type ProjectionFreshness = "loading" | "fresh" | "unavailable";

export interface ProjectionTransportState {
  freshness: ProjectionFreshness;
  projection: MainProjection | null;
  error: UiError | null;
}

export const initialProjectionTransportState: ProjectionTransportState = {
  freshness: "loading",
  projection: null,
  error: null,
};

export function projectionReadSucceeded(projection: MainProjection): ProjectionTransportState {
  return { freshness: "fresh", projection, error: null };
}

export function projectionReadFailed(error: UiError): ProjectionTransportState {
  return { freshness: "unavailable", projection: null, error };
}
