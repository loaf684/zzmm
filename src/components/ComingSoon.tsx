interface ComingSoonProps {
  phase: string;
  title: string;
  description: string;
}

/** Honest placeholder for features scheduled in later phases. */
export function ComingSoon({ phase, title, description }: ComingSoonProps) {
  return (
    <div className="mx-auto flex min-h-[70vh] max-w-2xl flex-col justify-center px-8">
      <p className="eyebrow">Phase {phase} // incoming</p>
      <h1 className="mt-1.5 font-display text-[26px] font-semibold tracking-tight text-zinc-100">
        {title}
      </h1>
      <p className="mt-3 text-sm leading-relaxed text-zinc-500">{description}</p>
      <div className="mt-8 h-px w-full bg-gradient-to-r from-acid-400/60 via-ink-600 to-transparent" />
    </div>
  );
}
