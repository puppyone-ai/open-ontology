import "@open-ontology/design-system/styles.css";
import "../../../src/renderer/app.css";
import { createRoot } from "react-dom/client";
import { Badge, Button, Card, Checkbox, EmptyState, Icon, IconButton, LinkButton, Notice, ProgressBar, Segmented, Select, Skeleton, Spinner, StatusDot, Switch, TextArea, TextField } from "@open-ontology/design-system";
import { ArrowRight, Database, FileText, FolderPlus, Plus, Trash2 } from "lucide-react";

const Section = ({ id, children }: { id: string; children: React.ReactNode }) => <section data-section={id} style={{ width: 560, padding: 16, margin: "0 0 16px", background: "var(--so-canvas)" }}>{children}</section>;
const Row = ({ children }: { children: React.ReactNode }) => <div className="so-row" data-wrap style={{ marginBottom: 12 }}>{children}</div>;

function Gallery() {
  return (
    <div style={{ padding: 16, background: "var(--so-canvas)" }}>
      <Section id="buttons">
        <Row><Button variant="primary">Primary</Button><Button>Secondary</Button><Button variant="ghost">Ghost</Button><Button variant="danger">Danger</Button><Button variant="quiet-danger">Quiet danger</Button></Row>
        <Row><Button variant="primary" icon={Plus}>With icon</Button><Button icon={FolderPlus}>With icon</Button><Button size="compact">Compact</Button><Button variant="primary" disabled>Disabled</Button><Button disabled>Disabled</Button></Row>
        <Row><Button variant="primary" loading icon={Plus} data-testid="loading-icon">With icon</Button><Button variant="primary" loading data-testid="loading-text">Connect</Button><Button loading data-testid="loading-secondary">Connect</Button><Button variant="primary" data-testid="idle-text">Connect</Button><Button variant="primary" icon={Plus} data-testid="idle-icon">With icon</Button></Row>
        <Row><LinkButton icon={ArrowRight}>All sources</LinkButton><IconButton icon={Trash2} label="Remove" /><IconButton icon={Trash2} label="Removing" loading data-testid="loading-icon-button" /><IconButton icon={Trash2} label="Remove" data-testid="idle-icon-button" variant="secondary" /></Row>
      </Section>
      <Section id="progress">
        <Row><Spinner size={12} /><Spinner size={16} /><Spinner size={20} /></Row>
        <Row><div style={{ width: 240 }}><ProgressBar value={0.4} label="Reading" /></div><div style={{ width: 240 }}><ProgressBar value={null} label="Reading" /></div></Row>
        <Row><StatusDot tone="success" label="Ready" /><StatusDot tone="warning" label="Needs attention" /><StatusDot tone="danger" label="Failed" /><StatusDot tone="accent" label="Syncing" /><StatusDot tone="neutral" label="Unknown" /></Row>
        <Row><Badge>Neutral</Badge><Badge tone="success">Reachable</Badge><Badge tone="warning">Draft</Badge><Badge tone="danger">Not reachable</Badge></Row>
      </Section>
      <Section id="fields">
        <div className="so-stack">
          <TextField label="Name" placeholder="For example, orders-db" helper="Letters, digits, - and _." />
          <TextField label="Host" defaultValue="db" error="This field is required." />
          <TextField label="Region" optionalLabel="Optional" helper="For example us-east-1." />
          <TextField label="Password" type="password" defaultValue="secret" helper="Kept in a private file on this computer." />
          <TextField label="Disabled" defaultValue="Locked" disabled />
          <TextArea label="Tables to read" placeholder="public.orders" helper="One per line, as schema.table." className="so-field-mono" />
          <Select label="Database type" defaultValue="postgres"><option value="postgres">PostgreSQL</option><option value="mysql">MySQL</option></Select>
        </div>
      </Section>
      <Section id="controls">
        <div className="so-stack">
          <Switch checked onChange={() => undefined} label="Keep the engine running" description="After the window closes." />
          <Switch checked={false} onChange={() => undefined} label="Send usage" />
          <Checkbox checked onChange={() => undefined} label="Read only" />
          <Segmented value="a" onChange={() => undefined} label="View" options={[{ value: "a", label: "List" }, { value: "b", label: "Grid" }]} />
        </div>
      </Section>
      <Section id="feedback">
        <div className="so-stack">
          <Notice tone="accent" title="Reading your files">It keeps going if you leave.</Notice>
          <Notice tone="warning" title="Model not set">Rules are used for everything.</Notice>
          <Notice tone="danger" title="Couldn't save the database" action={<Button size="compact">Retry</Button>}>The details were refused.</Notice>
          <Notice tone="success" title="Connected">Reachable.</Notice>
          <Card><EmptyState icon={Database} title="No sources yet" description="Add a folder, files, a bucket or a database." /></Card>
        </div>
      </Section>
      <Section id="rows">
        <Card className="so-list" data-testid="rows-real">
          <div className="so-list-row" data-testid="row-real"><span className="so-list-lead"><FileText size={16} strokeWidth={1.5} /></span><span className="so-list-text"><span className="so-list-title">Second docs</span><span className="so-list-sub">Folder · 1 file</span></span><StatusDot tone="success" label="Up to date" /></div>
        </Card>
        <div style={{ height: 12 }} />
        <Card className="so-list" data-testid="rows-skeleton">
          <div className="so-list-row" data-testid="row-skeleton" aria-hidden="true"><Skeleton width={32} height={32} radius={6} /><span className="so-list-text"><Skeleton width={140} height={13} /><Skeleton width={90} height={11} /></span></div>
        </Card>
        <div style={{ height: 12 }} />
        <div className="so-grid" data-cols="2"><Card className="so-metric" data-testid="metric-real"><span className="so-metric-head"><Icon icon={FileText} size={16} /><span className="so-metric-label">Files</span></span><span className="so-metric-value">12</span></Card><Card className="so-metric" data-testid="metric-skeleton"><span className="so-metric-head"><Icon icon={FileText} size={16} /><span className="so-metric-label">Files</span></span><span className="so-metric-value"><Skeleton width={48} height={24} /></span></Card></div>
      </Section>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Gallery />);
