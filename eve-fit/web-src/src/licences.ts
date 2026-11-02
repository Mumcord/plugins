/** CCP's proprietary notice (Developer License Agreement, section 7.1), shown below every fit. */
export const NOTICE =
  '© CCP hf. All rights reserved. "EVE", "EVE Online", "CCP", and all related logos and images are trademarks or registered trademarks of CCP hf. ' +
  "Ship, module and icon data and images are used under CCP's Developer License Agreement; this plugin is free and not affiliated with or endorsed by CCP hf. " +
  "Fitting wheel and statistics by EVEShip.fit (MIT).";

declare global {
  interface Window {
    /** Set by data/licences.js, which scripts/build.mjs writes from the bundled packages. */
    eveFitLicences?: string;
  }
}

/** The full licence texts of everything in the bundle. */
export function licences(): string {
  return window.eveFitLicences ?? "";
}
