import type { ModuleRegistration } from "../../app/module-registration";
import { overviewModules } from "./module";
import { sourceModules } from "../sources/module";
import { fileModules } from "../files/module";
import { jobModules } from "../jobs/module";
import { buildModules } from "../builds/module";
import { reviewModules } from "../reviews/module";
import { classificationModules } from "../classifications/module";
import { releaseModules } from "../releases/module";
import { ontologyModules } from "../ontology-catalog/module";
import { enumCalendarModules } from "../enums-calendar/module";
import { metricModules } from "../metrics/module";
import { entityTermModules } from "../entities-terms/module";
import { exampleModules } from "../examples/module";
import { permissionModules } from "../permissions/module";
import { modelSecretModules } from "../models-secrets/module";
import { settingsModules } from "../settings-integrations/module";
import { evaluationModules } from "../evaluations/module";
import { auditSessionModules } from "../audit-sessions/module";
import { operationModules } from "../operations/module";

export const ADMIN_FEATURE_MODULES: readonly ModuleRegistration[] = [
  ...overviewModules,
  ...sourceModules,
  ...fileModules,
  ...jobModules,
  ...buildModules,
  ...reviewModules,
  ...classificationModules,
  ...releaseModules,
  ...ontologyModules,
  ...enumCalendarModules,
  ...metricModules,
  ...entityTermModules,
  ...exampleModules,
  ...permissionModules,
  ...modelSecretModules,
  ...settingsModules,
  ...evaluationModules,
  ...auditSessionModules,
  ...operationModules,
];

export const ADMIN_PAGE_OPERATIONS: Readonly<Record<string, readonly string[]>> = {
  "A03": ["B02.sourceConfig","B03.jobs","B04.builder","B08.session","B10.suites"],
  "A04": ["B02.connectorMatrix","B02.sourceConfig"],
  "A05": ["B02.inspect","B02.catalog","B02.dba","B02.plan","B02.apply","B07.secretWrite"],
  "A06": ["B02.sourceConfig","B03.jobs"],
  "A07": ["B07.objectPart","B07.objectComplete","B07.objectAbort","B02.plan","B02.apply"],
  "A08": ["B07.download","B07.objectDelete","B03.jobs"],
  "A09": ["B03.jobs"],
  "A10": ["B03.checkpoints","B03.events","B03.cancel","B03.retry"],
  "A11": ["B04.builder"],
  "A12": ["B04.cards","B04.candidates","B03.jobs"],
  "A17": ["B04.reviews"],
  "A18": ["B04.reviews","B04.candidates"],
  "A19": ["B04.classifications","B06.roles"],
  "A29": ["B05.publish","B05.pack"],
  "A30": ["B05.gates","B05.publish","B03.jobs"],
  "A13": ["B04.candidates","B05.terms"],
  "A14": ["B04.candidates"],
  "A15": ["B04.candidates"],
  "A16": ["B05.terms"],
  "A20": ["B05.codeTables"],
  "A21": ["B05.calendar"],
  "A22": ["B05.metric"],
  "A23": ["B05.metric"],
  "A24": ["B05.entity"],
  "A25": ["B05.examples","B09.query"],
  "A26": ["B06.roles"],
  "A27": ["B01.projectRoleBinding","B06.bindings"],
  "A28": ["B06.permissionPreview","B05.pack"],
  "A31": ["B11.models","B07.secretWrite"],
  "A33": ["B07.secretWrite","B07.secretRotate"],
  "A32": ["B11.harness","B11.channels"],
  "A34": ["B11.deployment","B11.git","B11.config"],
  "A35": ["B06.oauth","B06.clientToken","B06.revoke","B11.plugin"],
  "A36": ["B11.channels"],
  "A37": ["B10.suites"],
  "A38": ["B10.run","B10.result","B10.compare","B10.privateHoldout"],
  "A39": ["B08.session","B12.audit"],
  "A40": ["B12.replay","B08.close"],
  "A41": ["B12.audit"],
  "A42": ["B12.health","B12.emergency"],
  "A43": ["B12.backup","B12.upgrade","B12.recovery"],
};
