import { bindingById, invokeOperation, type BindingRecord, type InvokeResult } from "@open-ontology/sdk-ts";

export type Consumption = {
  operationId: string;
  record: BindingRecord | null;
  result: InvokeResult;
};

export function consume(operationId: string): Consumption {
  return {
    operationId,
    record: bindingById(operationId),
    result: invokeOperation(operationId),
  };
}
