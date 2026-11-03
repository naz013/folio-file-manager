/** The third-party packages in the app, as `scripts/licenses.mjs` writes them. */

export interface LicensePackage {
  name: string;
  version: string;
  source: "crate" | "npm";
  /** SPDX expression, as the package declares it. */
  license: string;
  repo: string;
  /** Key into `texts`, or null when the package ships no license file. */
  text: string | null;
}

export interface LicenseData {
  packages: LicensePackage[];
  texts: Record<string, string>;
}

/** The packages whose name or license contains every word of `query`, in their order. */
export function filterPackages(
  packages: readonly LicensePackage[],
  query: string,
): LicensePackage[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [...packages];
  return packages.filter((p) => {
    const hay = `${p.name} ${p.license}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}
