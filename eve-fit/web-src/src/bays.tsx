/**
 * The drone bay and the cargo of the fit, read-only. EVEShip.fit lists them in a popover of the
 * bay buttons below its fitting wheel, which a read-only fitting window leaves out; here they
 * are always open below the wheel, with the same icons and quantities.
 */
import { useBayContents, useBayUsage } from "@eveshipfit/react-hooks";
import { StatsSection, TypeIcon } from "@eveshipfit/ui-ingame";

type BayName = "droneBay" | "cargo";

function Bay({ bay, title, lang }: { bay: BayName; title: string; lang: string }) {
  const contents = useBayContents(bay);
  const { used, total } = useBayUsage(bay);
  if (contents.length === 0) return null;
  const volume = new Intl.NumberFormat(lang, { maximumFractionDigits: 1 });
  const count = new Intl.NumberFormat(lang);
  // Its own box, so the sections are no siblings and both sit at the same height.
  return (
    <div>
      <StatsSection title={title} summary={`${volume.format(used)} / ${volume.format(total)} m³`} columns={1}>
        <ul className="bay" aria-label={title}>
          {contents.map(({ type, quantity }) => (
            <li key={type.id}>
              <TypeIcon typeId={type.id} size={32} />
              <span className="bay-name">{type.name}</span>
              <span className="bay-quantity">{count.format(quantity)}</span>
            </li>
          ))}
        </ul>
      </StatsSection>
    </div>
  );
}

export function Bays({ drones, cargo, lang }: { drones: string; cargo: string; lang: string }) {
  return (
    <div className="bays">
      <Bay bay="droneBay" title={drones} lang={lang} />
      <Bay bay="cargo" title={cargo} lang={lang} />
    </div>
  );
}
