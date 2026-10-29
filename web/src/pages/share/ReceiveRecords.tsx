// 「接收记录」：分享方、文件数与大小、开始时间、状态。快照只带最近 20 条，其余经 `agent.share.receiveRecords` 翻页。

import { Download, Flag } from "lucide-react";
import { useEffect, useState } from "react";
import { useT } from "../../i18n";
import { rpc } from "../../lib/rpc";
import type { ShareReceiveRecordDto } from "../../lib/rpc";
import {
  Badge,
  Button,
  EmptyState,
  Tooltip,
  toast,
  useIsMobile,
} from "../../ui";
import { accountErrorKey } from "../settings/sections/account/errorText";
import { formatBytes } from "../downloads/dialogs/utils";
import {
  RECEIVE_STATUS_KEYS,
  PROVIDE_STATE_KEYS,
  dateTimeLabel,
  mergeReceiveRecords,
} from "./model";
import { ReportDialog } from "./ReportDialog";
import { useShareRecords } from "./state";

const PAGE = 20;

const statusTone = (status: ShareReceiveRecordDto["status"]) =>
  status === "completed"
    ? "success"
    : status === "failed"
      ? "destructive"
      : "accent";

function ProvideCell({
  record,
  onStopped,
}: {
  record: ShareReceiveRecordDto;
  onStopped: (next: ShareReceiveRecordDto) => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const state = record.provideState ?? "off";
  const key = PROVIDE_STATE_KEYS[state];
  if (key === null) return null;
  return (
    <span className="inline-flex items-center gap-2">
      <Badge tone={state === "providing" ? "success" : "neutral"}>
        {t(key)}
      </Badge>
      {state === "providing" ? (
        <button
          type="button"
          disabled={busy}
          className="shrink-0 text-xs text-primary underline-offset-2 hover:underline disabled:opacity-50"
          onClick={() => {
            setBusy(true);
            rpc.agent.share
              .stopProviding({ recordId: record.id })
              .then(onStopped)
              .catch((error: unknown) => toast.error(t(accountErrorKey(error))))
              .finally(() => setBusy(false));
          }}
        >
          {t("shareStopProviding")}
        </button>
      ) : null}
    </span>
  );
}

function ReportButton({ record }: { record: ShareReceiveRecordDto }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  if (record.shareId === null) return null;
  return (
    <>
      <Tooltip content={t("shareReport")}>
        <Button
          variant="ghost"
          iconOnly
          aria-label={t("shareReport")}
          className="text-text-tertiary"
          onClick={() => setOpen(true)}
        >
          <Flag className="size-4" />
        </Button>
      </Tooltip>
      {open ? (
        <ReportDialog shareId={record.shareId} onClose={() => setOpen(false)} />
      ) : null}
    </>
  );
}

interface Paging {
  records: readonly ShareReceiveRecordDto[];
  cursor: string | null;
  loading: boolean;
}

export function ReceiveRecords() {
  const t = useT();
  const mobile = useIsMobile();
  const live = useShareRecords();
  const [paging, setPaging] = useState<Paging>({
    records: [],
    cursor: null,
    loading: true,
  });

  // 打开页签时取第一页：既拿到 `nextCursor`，也补上快照之外的更早记录。
  useEffect(() => {
    let cancelled = false;
    rpc.agent.share
      .receiveRecords({ limit: PAGE })
      .then((page) => {
        if (!cancelled)
          setPaging({
            records: page.records,
            cursor: page.nextCursor,
            loading: false,
          });
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setPaging((previous) => ({ ...previous, loading: false }));
        toast.error(t(accountErrorKey(error)));
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const loadMore = async () => {
    if (paging.cursor === null) return;
    setPaging((previous) => ({ ...previous, loading: true }));
    try {
      const page = await rpc.agent.share.receiveRecords({
        limit: PAGE,
        cursor: paging.cursor,
      });
      setPaging((previous) => ({
        records: [...previous.records, ...page.records],
        cursor: page.nextCursor,
        loading: false,
      }));
    } catch (error) {
      setPaging((previous) => ({ ...previous, loading: false }));
      toast.error(t(accountErrorKey(error)));
    }
  };

  const stopped = (next: ShareReceiveRecordDto) =>
    setPaging((previous) => ({
      ...previous,
      records: previous.records.map((record) =>
        record.id === next.id ? next : record,
      ),
    }));
  const records = mergeReceiveRecords(live, paging.records);
  if (records.length === 0) {
    return <EmptyState icon={Download} title={t("shareReceivedEmpty")} />;
  }

  return (
    <div className="flex flex-col gap-3">
      {mobile ? (
        <ul className="flex flex-col gap-2">
          {records.map((record) => (
            <li
              key={record.id}
              className="flex min-w-0 flex-col gap-1 rounded-md border border-hairline bg-surface p-3"
            >
              <div className="flex min-w-0 items-start gap-2">
                <span className="min-w-0 flex-1 break-words text-sm text-foreground">
                  {record.title}
                </span>
                <Badge tone={statusTone(record.status)}>
                  {t(RECEIVE_STATUS_KEYS[record.status])}
                </Badge>
              </div>
              <div className="tabular text-xs text-muted-foreground">
                {record.senderDeviceName === ""
                  ? t("shareSenderUnknown")
                  : record.senderDeviceName}{" "}
                · {t("shareFilesCount", { count: record.fileCount })} ·{" "}
                {formatBytes(record.totalBytes)}
              </div>
              <div className="tabular text-xs text-text-tertiary">
                {dateTimeLabel(record.startedAt)}
              </div>
              <div className="flex items-center justify-between gap-2">
                <ProvideCell record={record} onStopped={stopped} />
                <ReportButton record={record} />
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <div className="min-w-0 overflow-x-auto rounded-md border border-hairline">
          <table className="w-full min-w-[720px] text-left text-sm">
            <thead className="bg-chrome text-xs text-text-tertiary">
              <tr>
                <th className="px-3 py-2 font-normal">{t("shareColFile")}</th>
                <th className="px-3 py-2 font-normal">{t("shareColSender")}</th>
                <th className="px-3 py-2 font-normal">{t("shareColSize")}</th>
                <th className="px-3 py-2 font-normal">
                  {t("shareColStarted")}
                </th>
                <th className="px-3 py-2 font-normal">{t("shareColStatus")}</th>
                <th className="px-3 py-2 font-normal">
                  {t("shareColProvide")}
                </th>
                <th className="w-12 px-2 py-2" aria-label={t("shareReport")} />
              </tr>
            </thead>
            <tbody>
              {records.map((record) => (
                <tr key={record.id} className="border-t border-hairline">
                  <td className="max-w-[320px] px-3 py-2">
                    <div
                      className="truncate text-foreground"
                      title={record.title}
                    >
                      {record.title}
                    </div>
                    <div className="text-xs text-text-tertiary">
                      {t("shareFilesCount", { count: record.fileCount })}
                    </div>
                  </td>
                  <td
                    className="max-w-[200px] truncate px-3 py-2 text-xs text-muted-foreground"
                    title={record.senderDeviceName}
                  >
                    {record.senderDeviceName === ""
                      ? t("shareSenderUnknown")
                      : record.senderDeviceName}
                  </td>
                  <td className="tabular px-3 py-2 text-xs text-muted-foreground">
                    {formatBytes(record.totalBytes)}
                  </td>
                  <td className="tabular px-3 py-2 text-xs text-muted-foreground">
                    {dateTimeLabel(record.startedAt)}
                  </td>
                  <td className="px-3 py-2">
                    <Badge tone={statusTone(record.status)}>
                      {t(RECEIVE_STATUS_KEYS[record.status])}
                    </Badge>
                  </td>
                  <td className="px-3 py-2">
                    <ProvideCell record={record} onStopped={stopped} />
                  </td>
                  <td className="px-2 py-1 text-right">
                    <ReportButton record={record} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {paging.cursor !== null ? (
        <div className="flex justify-center">
          <Button loading={paging.loading} onClick={() => void loadMore()}>
            {t("shareLoadMore")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
