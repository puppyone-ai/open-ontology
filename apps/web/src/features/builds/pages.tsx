import { useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { parseBuildList, type CallResult } from "@open-ontology/sdk-ts";
import { useIdentityView } from "../../app/identity-context";
import { StatePanel } from "../../components/shared/state-panel";
import { BuildStages } from "../admin-overview/blocks";
import { AdminSurface } from "../admin-overview/surface";
import { adminProjectId, BUILD_ID, loadBuildDetail, loadBuildList, readBuildRows, readFlag, readNextCursor } from "./build-list";

export function BuildsPage() {
  return (
    <AdminSurface
      title="构建"
      description="Self-build 的来源、模型版本和范围都还没有运行。"
      groups={["B04"]}
      operationIds={["B04.builder", "createBuild", "listBuilds"]}
      action={{ id: "a11", label: "运行构建" }}
      notes={["没有模型调用。失败或未运行都不会产生审核或发布。"]}
    >
      <BuildListRead />
    </AdminSurface>
  );
}

export function BuildDetailPage() {
  return (
    <AdminSurface
      title="构建详情"
      description="S0 到 S10、候选、缓存、用量和费用都还没有记录。"
      groups={["B04", "B03"]}
      operationIds={["B04.cards", "B04.candidates", "B03.jobs", "getBuild", "cancelBuild", "getBuildCards", "getBuildCandidates", "compileBuildPack", "getBuildPack"]}
      action={{ id: "a12", label: "重试" }}
      notes={["没有用量，也没有费用。模型自评置信度没有数值。", "这个地址不代表已经存在的对象。"]}
    >
      <BuildDetailRead />
      <BuildStages />
    </AdminSurface>
  );
}

function BuildListRead() {
  const view = useIdentityView();
  const projectId = view.status === "session" ? adminProjectId(view.session) : null;
  const [result, setResult] = useState<CallResult | null>(null);
  useEffect(() => {
    if (!projectId) {
      return;
    }
    let cancelled = false;
    void loadBuildList(projectId).then((next) => {
      if (!cancelled) {
        setResult(next);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [projectId]);
  if (view.status !== "session") {
    return <p>当前身份尚未接入。</p>;
  }
  if (!projectId) {
    return <p>还没有项目能力。</p>;
  }
  if (!result) {
    return <StatePanel kind="loading" />;
  }
  if (!result.ok) {
    return (
      <p>
        listBuilds {result.reason}
        {result.errorCode ? ` ${result.errorCode}` : ""}
      </p>
    );
  }
  const parsed = parseBuildList(result.body);
  if (!parsed.ok) {
    return <p>listBuilds invalid_schema</p>;
  }
  const cursor = readNextCursor(result.body);
  return (
    <>
      <p>listBuilds {parsed.count}</p>
      <p>published {readFlag(result.body, "published")}</p>
      <p>active {readFlag(result.body, "active")}</p>
      <p>compiled_pack {readFlag(result.body, "compiled_pack")}</p>
      {parsed.count === 0 ? (
        <>
          <p>没有构建行。</p>
          <p>空列表不是完成的构建。</p>
        </>
      ) : null}
      <ul>
        {readBuildRows(result.body).map((row) => (
          <li key={row.buildId}>
            <Link to={`/admin/builds/${row.buildId}`}>{row.buildId}</Link> {row.status} {row.stage}
          </li>
        ))}
      </ul>
      {cursor ? <p>next_cursor {cursor}</p> : null}
      <p>审核和分类还没有读取。</p>
    </>
  );
}

function BuildDetailRead() {
  const { id } = useParams();
  const buildId = id && BUILD_ID.test(id) ? id : null;
  const [result, setResult] = useState<CallResult | null>(null);
  useEffect(() => {
    if (!buildId) {
      return;
    }
    let cancelled = false;
    void loadBuildDetail(buildId).then((next) => {
      if (!cancelled) {
        setResult(next);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [buildId]);
  if (!buildId) {
    return <p>审核和分类还没有读取。</p>;
  }
  if (!result) {
    return <StatePanel kind="loading" />;
  }
  if (!result.ok) {
    return (
      <>
        <p>
          getBuild {result.reason}
          {result.errorCode ? ` ${result.errorCode}` : ""}
        </p>
        <p>审核和分类还没有读取。</p>
      </>
    );
  }
  return (
    <>
      <p>getBuild {result.status} live false</p>
      <p>审核和分类还没有读取。</p>
    </>
  );
}
