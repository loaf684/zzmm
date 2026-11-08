import { useState } from "react";
import { Loader2 } from "lucide-react";

import { Panel } from "@/components/ui/Panel";
import { Switch } from "@/components/ui/Switch";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import type { Settings } from "@/types/settings";

interface BehaviorSectionProps {
  settings: Settings;
}

export function BehaviorSection({ settings }: BehaviorSectionProps) {
  const save = useSettingsStore((state) => state.save);
  const isSaving = useSettingsStore((state) => state.isSaving);
  const [pending, setPending] = useState(false);

  const toggle = async (checked: boolean) => {
    setPending(true);
    const saved = await save({
      ...settings,
      oneActivePerCharacter: checked,
    });
    setPending(false);

    if (saved) {
      toast.success(
        "Behavior updated",
        checked
          ? "Enabling a mod now disables other enabled mods for the same character."
          : "Mods can be enabled independently again.",
      );
    }
  };

  return (
    <Panel
      eyebrow="Behavior"
      title="Mod switching"
      description="Optional safeguards for characters with several outfits installed at once."
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-zinc-200">
            One active mod per character
          </p>
          <p className="mt-1 max-w-2xl text-[12px] leading-relaxed text-zinc-500">
            When you enable a mod, other enabled mods for the same character are
            disabled automatically. Mods grouped as{" "}
            <span className="text-zinc-400">Uncategorized</span> are never
            touched. A snapshot of the previous state is written before each
            batch change.
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {pending ? (
            <Loader2
              className="h-3.5 w-3.5 animate-spin text-acid-400"
              aria-hidden
            />
          ) : null}
          <Switch
            checked={settings.oneActivePerCharacter}
            onCheckedChange={(checked) => void toggle(checked)}
            disabled={pending || isSaving}
            label="One active mod per character"
          />
        </div>
      </div>
    </Panel>
  );
}
