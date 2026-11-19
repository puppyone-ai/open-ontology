import { useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Dialog, Menu, MenuItem, MenuSeparator, Notice, Select, TextField } from "@open-ontology/design-system";
import { Cloud, Database, FileUp, Folder, Plus } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { useSuperDriveActions } from "../../model/SuperDriveProvider";
import { getUploadQueue } from "../../model/uploadStore";
import { EMPTY_S3_FORM, isAllowedEndpoint, missingS3Fields, objectStoreFields, toS3CreateInput, type S3FieldName } from "../../model/settings/s3Form";
import type { ObjectStoreBackend } from "../../model/superDriveApi";
import type { SuperDriveError } from "../../model/superDriveErrors";
import { createOperationId } from "../../model/operationIds";
import { AddDatabaseDialog } from "./AddDatabase";

/** The one way in: files into "My files" (no setup), a folder on this computer, a bucket, or (CLI) a database. */
export function useAddFolder(onOpened?: (driveId: string) => void) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const [error, setError] = useState<SuperDriveError | null>(null);
  const addFolder = async () => {
    const picked = await actions.pickDriveRoot();
    if (!picked.ok) { setError(picked.error); return; }
    if ("cancelled" in picked.value) return;
    const name = picked.value.displayPath.split("/").filter(Boolean).pop() ?? t("app.sources.folder.defaultName");
    const created = await actions.createDrive({ name, rootGrantId: picked.value.grantId });
    if (!created.ok) { setError(created.error); return; }
    setError(null);
    onOpened?.(created.value.id);
  };
  return { addFolder, error };
}

export function AddSourceMenu({ onOpened, onDatabaseAdded }: { onOpened?: (driveId: string) => void; onDatabaseAdded?: (name: string) => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const anchor = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [cloud, setCloud] = useState(false);
  const [database, setDatabase] = useState(false);
  const [fileError, setError] = useState<SuperDriveError | null>(null);
  const folder = useAddFolder(onOpened);
  const error = fileError ?? folder.error;
  const errorText = useErrorText();

  const addFiles = async (picked: FileList | null) => {
    // The input is cleared right after this call: keep the File objects, not the live list.
    const files = picked ? Array.from(picked) : [];
    if (files.length === 0) return;
    const drive = await actions.ensureMyFiles();
    if (!drive.ok) { setError(drive.error); return; }
    setError(null);
    await getUploadQueue().enqueueFiles(drive.value.id, "", files);
    onOpened?.(drive.value.id);
  };
  return (
    <>
      <Button ref={anchor} variant="primary" icon={Plus} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((v) => !v)} data-testid="add-source">{t("app.sources.add")}</Button>
      <input ref={input} type="file" multiple hidden onChange={(event) => { void addFiles(event.target.files); event.target.value = ""; }} />
      <Menu anchor={anchor} open={open} onClose={() => setOpen(false)} ariaLabel={t("app.sources.add")}>
        <MenuItem icon={FileUp} label={t("app.sources.add.files")} closeMenu={() => setOpen(false)} onSelect={() => input.current?.click()} testId="add-source-files" />
        <MenuItem icon={Folder} label={t("app.sources.add.folder")} closeMenu={() => setOpen(false)} onSelect={() => { void folder.addFolder(); }} testId="add-source-folder" />
        <MenuItem icon={Cloud} label={t("app.sources.add.cloud")} closeMenu={() => setOpen(false)} onSelect={() => setCloud(true)} testId="add-source-cloud" />
        <MenuSeparator />
        <MenuItem icon={Database} label={t("app.sources.add.database")} closeMenu={() => setOpen(false)} onSelect={() => setDatabase(true)} testId="add-source-database" />
      </Menu>
      {error && <div className="so-visually-hidden" role="alert">{errorText(error)}</div>}
      {database && <AddDatabaseDialog onClose={() => setDatabase(false)} onAdded={(name) => { setDatabase(false); onDatabaseAdded?.(name); }} />}
      {cloud && <CloudStorageDialog onClose={() => setCloud(false)} onCreated={(id) => { setCloud(false); onOpened?.(id); }} />}
    </>
  );
}

const PROVIDERS: ReadonlyArray<{ value: ObjectStoreBackend; label: string }> = [
  { value: "s3", label: "Amazon S3 / S3-compatible" }, { value: "oss", label: "Alibaba Cloud OSS" }, { value: "cos", label: "Tencent Cloud COS" }, { value: "r2", label: "Cloudflare R2" },
];

export function CloudStorageDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (driveId: string) => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const [backend, setBackend] = useState<ObjectStoreBackend>("s3");
  const [name, setName] = useState("");
  const [values, setValues] = useState(EMPTY_S3_FORM);
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const fields = objectStoreFields(backend);
  const missing = new Set<S3FieldName>(missingS3Fields(values, backend));
  const set = (patch: Partial<typeof values>) => setValues((current) => ({ ...current, ...patch }));
  const required = t("app.sources.cloud.required");
  const err = (field: S3FieldName) => (touched && missing.has(field) ? required : null);
  const endpointBad = values.endpoint !== "" && !isAllowedEndpoint(values.endpoint);

  const submit = async () => {
    setTouched(true);
    if (missing.size > 0 || name.trim() === "" || endpointBad) return;
    setBusy(true);
    setError(null);
    const result = await actions.createDrive(toS3CreateInput(name.trim(), values, createOperationId("dr"), backend));
    setBusy(false);
    if (!result.ok) { setError(result.error); return; }
    onCreated(result.value.id);
  };

  return (
    <Dialog title={t("app.sources.cloud.title")} description={t("app.sources.cloud.description")} onClose={busy ? undefined : onClose} closeLabel={t("app.common.close")}
      footer={<><Button variant="ghost" onClick={onClose} disabled={busy} data-testid="add-source-cancel">{t("app.common.cancel")}</Button><Button variant="primary" loading={busy} onClick={() => { void submit(); }} data-testid="add-source-confirm">{t("app.sources.cloud.connect")}</Button></>}>
      {error && <Notice tone="danger" title={t("app.sources.cloud.failed")} testId="add-source-error">{errorText(error)}</Notice>}
      <Select label={t("app.sources.cloud.provider")} value={backend} onChange={(event) => setBackend(event.target.value as ObjectStoreBackend)}>
        {PROVIDERS.map((p) => <option key={p.value} value={p.value}>{p.label}</option>)}
      </Select>
      <TextField label={t("app.sources.cloud.name")} value={name} onChange={(e) => setName(e.target.value)} error={touched && name.trim() === "" ? required : null} helper={t("app.sources.cloud.name.help")} placeholder={t("app.sources.cloud.name.placeholder")} autoFocus maxLength={128} />
      <TextField label={t("app.sources.cloud.bucket")} value={values.bucket} onChange={(e) => set({ bucket: e.target.value })} error={err("bucket")} spellCheck={false} helper={t("app.sources.cloud.bucket.help")} />
      {fields.region !== "none" && <TextField label={t("app.sources.cloud.region")} value={values.region} onChange={(e) => set({ region: e.target.value })} error={err("region")} spellCheck={false} helper={t("app.sources.cloud.region.help")} optionalLabel={fields.region === "optional" ? t("app.sources.cloud.optional") : undefined} />}
      {fields.accountId && <TextField label={t("app.sources.cloud.accountId")} value={values.accountId} onChange={(e) => set({ accountId: e.target.value })} error={err("accountId")} spellCheck={false} helper={t("app.sources.cloud.accountId.help")} />}
      <TextField label={t("app.sources.cloud.endpoint")} value={values.endpoint} onChange={(e) => set({ endpoint: e.target.value })} error={endpointBad ? t("app.sources.cloud.endpoint.bad") : err("endpoint")} placeholder="https://" spellCheck={false} optionalLabel={fields.endpoint === "optional" ? t("app.sources.cloud.optional") : undefined} />
      <TextField label={t("app.sources.cloud.prefix")} value={values.prefix} onChange={(e) => set({ prefix: e.target.value })} spellCheck={false} optionalLabel={t("app.sources.cloud.optional")} helper={t("app.sources.cloud.prefix.help")} />
      <div className="so-grid" data-cols="2">
        <TextField label={t("app.sources.cloud.accessKey")} value={values.accessKeyId} onChange={(e) => set({ accessKeyId: e.target.value })} error={err("accessKeyId")} spellCheck={false} autoComplete="off" helper={t("app.sources.cloud.accessKey.help")} />
        <TextField label={t("app.sources.cloud.secretKey")} type="password" value={values.secretAccessKey} onChange={(e) => set({ secretAccessKey: e.target.value })} error={err("secretAccessKey")} autoComplete="off" helper={t("app.sources.cloud.secretKey.help")} />
      </div>
      <p className="so-weak">{t("app.sources.cloud.readonly")}</p>
    </Dialog>
  );
}
