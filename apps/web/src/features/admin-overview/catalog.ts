import type { ModuleRegistration } from "../../app/module-registration";
import { overviewModules } from "./module";
import { sourceModules } from "../sources/module";
import { fileModules } from "../files/module";
import { jobModules } from "../jobs/module";
import { buildModules } from "../builds/module";
import { reviewModules } from "../reviews/module";
import { classificationModules } from "../classifications/module";
import { releaseModules } from "../releases/module";

export const ADMIN_FEATURE_MODULES: readonly ModuleRegistration[] = [
  ...overviewModules,
  ...sourceModules,
  ...fileModules,
  ...jobModules,
  ...buildModules,
  ...reviewModules,
  ...classificationModules,
  ...releaseModules,
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
};
