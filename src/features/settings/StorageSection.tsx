import type { LucideIcon } from "lucide-react";
import { FileJson, HardDrive, ScrollText } from "lucide-react";

import { Panel } from "@/components/ui/Panel";
import type { AppPaths } from "@/types/settings";

interface StorageSectionProps {
  paths: AppPaths;
}

export function StorageSection({ paths }: StorageSectionProps) {
  return (
    <Panel
      eyebrow="Local data"
      title="Storage & logs"
      description="Configuration and logs stay on this machine. ZZMM never uploads telemetry."
    >
      <dl className="space-y-3.5">
        <PathRow icon={FileJson} label="Settings file" value={paths.settingsFile} />
        <PathRow icon={ScrollText} label="Log folder" value={paths.logDir} />
        <PathRow
          icon={HardDrive}
          label="App version"
          value={`ZZMM ${paths.appVersion} · Tauri 2`}
        />
      </dl>
    </Panel>
  );
}

interface PathRowProps {
  icon: LucideIcon;
  label: string;
  value: string;
}

function PathRow({ icon: Icon, label, value }: PathRowProps) {
  return (
    <div className="flex items-start gap-3">
      <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-zinc-600" aria-hidden />
      <div className="min-w-0">
        <dt className="font-display text-[10px] uppercase tracking-[0.18em] text-zinc-500">
          {label}
        </dt>
        <dd className="mt-0.5 break-all font-mono text-[11px] text-zinc-300">
          {value}
        </dd>
      </div>
    </div>
  );
}
