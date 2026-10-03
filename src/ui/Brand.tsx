/** Renders View V from the shared SVG master with interface-owned colors. */
import symbolUrl from "../assets/brand/view-v-symbol.svg?url&no-inline";
import "./brand.scss";


export function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <span className={`gitview-brand${compact ? " gitview-brand--compact" : ""}`} role="img" aria-label="GitView">
      <svg className="gitview-brand-mark" viewBox="0 0 64 64" aria-hidden="true" focusable="false">
        <use className="gitview-brand-frame" href={`${symbolUrl}#frame`} />
        <use className="gitview-brand-branch" href={`${symbolUrl}#branch`} />
      </svg>
      {compact ? null : <span className="gitview-brand-wordmark" aria-hidden="true">GitView</span>}
    </span>
  );
}
