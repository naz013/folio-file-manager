<script lang="ts">
  import { onMount } from "svelte";
  import {
    cleanupDuplicates,
    cleanupDuplicatesCancel,
    cleanupLargeOld,
    cleanupOldDownloads,
    cleanupPartials,
    cleanupTemp,
    cleanupTrash,
    cleanupTrashDuplicates,
    cleanupTrashPartials,
    cleanupTrashTemp,
    listDrives,
    onQueueUpdate,
    parentOf,
    scanRoots,
    type BigFile,
    type DuplicateGroup,
    type Drive,
    type PartialCopy,
    type TempFile,
    type TempPlace,
  } from "$lib/api/fs";
  import { formatDate, formatSize } from "$lib/format";
  import {
    CLEANUP_AGES_DAYS,
    DOWNLOAD_AGES_DAYS,
    DUPLICATE_SIZES_MB,
    PARTIAL_AGES_DAYS,
    CLEANUP_SIZES_MB,
    settings,
    updateSettings,
  } from "$lib/settings.svelte";
  import EmptyState from "./EmptyState.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  let {
    start = "large",
    onopen,
  }: {
    /** Which list to open on. */
    start?: "large" | "downloads" | "duplicates" | "partials" | "temp";
    /** Show this folder in the browser. */
    onopen: (path: string) => void;
  } = $props();

  /** The list shows this many suggestions, the biggest first. The cap is why the rows are
   * not virtualized: the list is bounded, and `content-visibility` skips the off-screen ones. */
  const LIMIT = 200;

  let dialog: HTMLElement;

  /** The two plain lists, kept while the person looks at another one, so what was picked
   * in them counts toward the move. */
  let lists = $state.raw<{
    large: BigFile[];
    downloads: BigFile[];
    partials: PartialCopy[];
    temp: TempFile[];
  }>({
    large: [],
    downloads: [],
    partials: [],
    temp: [],
  });
  /** Where temp files and caches are looked for: the whole rule, shown so it can be checked. */
  let tempPlaces = $state.raw<TempPlace[]>([]);
  /** False if a place was too big to look through whole. */
  let tempComplete = $state(true);
  let drives = $state.raw<Drive[]>([]);
  let moving = $state(false);
  /** What an empty list of suggestions says, by what was looked for. */
  function nothingFound(): string {
    if (mode === "temp") {
      return "No temp file or cache in the places below has been left alone long enough. A file whose age can't be told is never listed.";
    }
    if (mode === "partials") {
      return `No unfinished copy has been left alone for ${ageLabel(settings.cleanupPartialDays)}, among the scanned folders and files of 1 MB or more.`;
    }
    if (mode === "downloads") {
      return `No installer or archive in Downloads has been untouched for ${ageLabel(settings.cleanupDownloadsDays)}.`;
    }
    return `No file is bigger than ${sizeLabel(settings.cleanupMinMb)} and untouched for ${ageLabel(settings.cleanupAgeDays)}.`;
  }

  /** Nothing is picked until the person picks it: cleanup only suggests (ADR 0003). */
  const picked = $state<Record<string, boolean>>({});
  let loading = $state(true);
  let scannedAny = $state(true);
  let error = $state("");
  let loadId = 0;
  // `start` only picks the list the view opens on; after that the person switches it.
  // What is looked for: big files nobody touches, or installers and archives in Downloads.
  // svelte-ignore state_referenced_locally
  let mode = $state<"large" | "downloads" | "duplicates" | "partials" | "temp">(start);
  const files = $derived(mode === "duplicates" ? [] : lists[mode]);
  /** The Recycle Bin jobs started from here; the lists refresh when they have all ended. */
  let trashJobs = $state<number[]>([]);
  /** Jobs seen ending, so one that ends before its id is known is not waited for. */
  const ended: Record<number, true> = {};
  const busy = $derived(moving || trashJobs.length > 0);
  /** Which categories the move includes. */
  // Duplicates join the move once the person has looked at them (opened that list).
  // svelte-ignore state_referenced_locally
  const included = $state({
    large: true,
    downloads: true,
    duplicates: start === "duplicates",
    partials: start === "partials",
    temp: start === "temp",
  });
  /** What each list was last loaded for, so switching back doesn't search again. */
  const loadedFor: Record<string, string> = {};

  const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const sizeLabel = (mb: number) => (mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`);
  const ageLabel = (days: number) =>
    days >= 365 ? `${days / 365} ${days === 365 ? "year" : "years"}` : `${days} days`;

  /** Duplicate groups, each with the copy to keep (the newest, until another is chosen) and
   * whether the person left the group alone. */
  let groups = $state.raw<DuplicateGroup[]>([]);
  let complete = $state(true);
  let unreadable = $state(0);
  const keep = $state<Record<string, string>>({});
  const skipped = $state<Record<string, boolean>>({});
  const keptIn = (g: DuplicateGroup) => keep[g.id] ?? g.files[0].path;
  /** What would move: in each group not left alone, every copy but the kept one. */
  const dupRemoval = $derived(
    groups
      .filter((g) => !skipped[g.id])
      .map((g) => ({
        files: g.files,
        remove: g.files.filter((f) => f.path !== keptIn(g)).map((f) => f.path),
      })),
  );

  type Category = {
    id: "large" | "downloads" | "duplicates" | "partials" | "temp";
    label: string;
    count: number;
    bytes: number;
  };

  /** What one click moves, across all the lists: the files picked in the two plain lists
   * (each counted once), and the copies to go from the duplicate groups. A file that is the
   * copy kept in a group never goes with the plain lists, so a group can't lose its last
   * copy through another list, while that group is included and its search has run. */
  const plan = $derived.by(() => {
    const kept: Record<string, true> = {};
    const going: Record<string, true> = {};
    if (included.duplicates) {
      for (const g of dupRemoval) {
        for (const f of g.files) {
          if (g.remove.includes(f.path)) going[f.path] = true;
          else kept[f.path] = true;
        }
      }
    }
    const taken: Record<string, true> = {};
    let heldBack = 0;
    const take = <T extends BigFile>(list: T[]) =>
      list.filter((f) => {
        if (!picked[f.path] || taken[f.path] || going[f.path]) return false;
        if (kept[f.path]) {
          heldBack += 1;
          return false;
        }
        taken[f.path] = true;
        return true;
      });
    const large = included.large ? take(lists.large) : [];
    const downloads = included.downloads ? take(lists.downloads) : [];
    const partials = included.partials ? take(lists.partials) : [];
    const temp = included.temp ? take(lists.temp) : [];
    const singles = [...large, ...downloads];
    const dupGroups = included.duplicates ? dupRemoval.filter((g) => g.remove.length) : [];
    const dupFiles = dupGroups.flatMap((g) => g.files.filter((f) => g.remove.includes(f.path)));
    const sum = (list: BigFile[]) => list.reduce((n, f) => n + f.size, 0);
    const categories: Category[] = [
      { id: "large", label: "Large & old files", count: large.length, bytes: sum(large) },
      { id: "downloads", label: "Old downloads", count: downloads.length, bytes: sum(downloads) },
      { id: "duplicates", label: "Duplicates", count: dupFiles.length, bytes: sum(dupFiles) },
      { id: "partials", label: "Unfinished copies", count: partials.length, bytes: sum(partials) },
      { id: "temp", label: "Temp files & caches", count: temp.length, bytes: sum(temp) },
    ];
    const all = [...singles, ...dupFiles, ...partials, ...temp];
    // Per drive, so the space it gives back can be shown against what it has free.
    const byDrive: Record<string, number> = {};
    for (const f of all) {
      const letter = /^[a-z]:/i.test(f.path) ? f.path.slice(0, 2).toUpperCase() : "Other locations";
      byDrive[letter] = (byDrive[letter] ?? 0) + f.size;
    }
    return {
      singles,
      partials,
      temp,
      dupGroups,
      categories,
      count: all.length,
      bytes: sum(all),
      byDrive,
      heldBack,
      keptPaths: Object.keys(kept),
    };
  });
  const chosenCount = $derived(plan.count);
  const chosenBytes = $derived(plan.bytes);
  /** What each drive would have free once the Recycle Bin is emptied. */
  const spaceAfter = $derived(
    Object.entries(plan.byDrive).map(([letter, bytes]) => {
      const drive = drives.find((d) => d.path.slice(0, 2).toUpperCase() === letter);
      return { letter, bytes, free: drive?.free ?? null };
    }),
  );
  const totalBytes = $derived(files.reduce((sum, f) => sum + f.size, 0));

  /** What the current list depends on: when it changes, the list is looked up again. */
  const settingsKey = (list = mode) =>
    JSON.stringify(
      list === "large"
        ? [settings.cleanupMinMb, settings.cleanupAgeDays]
        : list === "downloads"
          ? [settings.cleanupDownloadsDays]
          : list === "partials"
            ? [settings.cleanupPartialDays]
            : list === "temp"
              ? []
              : [settings.cleanupDupMinMb],
    );

  async function load() {
    const id = ++loadId;
    // What this call is for; the person may switch lists or change a setting while it runs.
    const target = mode;
    const key = settingsKey();
    // A search for duplicates still running is of no use to a list that replaces it.
    if (target !== "duplicates") cleanupDuplicatesCancel().catch(() => {});
    loading = true;
    error = "";
    try {
      if (target === "duplicates") {
        const [found, roots] = await Promise.all([
          cleanupDuplicates(settings.cleanupDupMinMb * 1024 * 1024),
          scanRoots().catch(() => []),
        ]);
        if (id !== loadId) return;
        groups = found.groups;
        complete = found.complete;
        unreadable = found.unreadable;
        scannedAny = roots.length > 0;
        // Choices only stay for groups that are still there.
        for (const key of Object.keys(keep)) {
          const g = found.groups.find((x) => x.id === key);
          if (!g || !g.files.some((f) => f.path === keep[key])) delete keep[key];
        }
        for (const key of Object.keys(skipped)) {
          if (!found.groups.some((x) => x.id === key)) delete skipped[key];
        }
        loadedFor.duplicates = key;
        return;
      }
      if (target === "temp") {
        // A short fixed list of places, read directly: no scan is needed.
        const found = await cleanupTemp(LIMIT);
        if (id !== loadId) return;
        lists = { ...lists, temp: found.files };
        tempPlaces = found.places;
        tempComplete = found.complete;
        scannedAny = true;
        loadedFor.temp = key;
        for (const path of Object.keys(picked)) {
          if (
            ![...lists.large, ...lists.downloads, ...lists.partials, ...lists.temp].some(
              (f) => f.path === path,
            )
          )
            delete picked[path];
        }
        return;
      }
      const [found, roots] = await Promise.all([
        target === "large"
          ? cleanupLargeOld(settings.cleanupMinMb * 1024 * 1024, settings.cleanupAgeDays, LIMIT)
          : target === "partials"
            ? cleanupPartials(settings.cleanupPartialDays, LIMIT)
            : cleanupOldDownloads(settings.cleanupDownloadsDays, LIMIT),
        scanRoots().catch(() => []),
      ]);
      if (id !== loadId) return;
      lists = { ...lists, [target]: found };
      // Only the large-file list comes from the scan; Downloads is read directly.
      scannedAny = target === "downloads" || roots.length > 0;
      loadedFor[target] = key;
      // Only what is still listed in one of the lists can stay picked.
      for (const path of Object.keys(picked)) {
        if (
          ![...lists.large, ...lists.downloads, ...lists.partials, ...lists.temp].some(
            (f) => f.path === path,
          )
        )
          delete picked[path];
      }
    } catch (e) {
      if (id === loadId) {
        error = String(e);
        // What could not be looked at has no list, not an old one.
        if (target !== "duplicates") lists = { ...lists, [target]: [] };
      }
    } finally {
      if (id === loadId) loading = false;
    }
  }

  // The thresholds changed: look again.
  $effect(() => {
    void settings.cleanupMinMb;
    void settings.cleanupAgeDays;
    void settings.cleanupDownloadsDays;
    void settings.cleanupDupMinMb;
    void settings.cleanupPartialDays;
    void mode;
    // Back on a list already looked up for these settings: nothing to do.
    if (loadedFor[mode] !== settingsKey()) load();
  });

  /** Everything is out of date once files have moved: forget the lists and look again. */
  async function refreshAll() {
    lists = { large: [], downloads: [], partials: [], temp: [] };
    groups = [];
    for (const key of Object.keys(keep)) delete keep[key];
    for (const key of Object.keys(skipped)) delete skipped[key];
    for (const key of Object.keys(loadedFor)) delete loadedFor[key];
    listDrives()
      .then((d) => (drives = d))
      .catch(() => {});
    await load();
  }

  async function trash() {
    if (busy || !plan.count) return;
    // What the button promised, taken before anything is sent; the inputs are locked while
    // it runs.
    const { singles, partials, temp, dupGroups, heldBack, keptPaths } = plan;
    moving = true;
    error = "";
    const jobs: number[] = [];
    let leftAlone = heldBack;
    let failure = "";
    try {
      // The core moves only files still exactly as listed and still allowed, as normal
      // Recycle Bin jobs (so they can be undone), one after another. For duplicates it also
      // makes sure a copy of each group stays.
      if (singles.length) {
        const done = await cleanupTrash(singles, keptPaths);
        leftAlone += done.leftAlone;
        if (done.job !== null) jobs.push(done.job);
        for (const f of singles) delete picked[f.path];
      }
      if (partials.length) {
        const done = await cleanupTrashPartials(partials);
        leftAlone += done.leftAlone;
        if (done.job !== null) jobs.push(done.job);
        for (const f of partials) delete picked[f.path];
      }
      if (temp.length) {
        const done = await cleanupTrashTemp(temp);
        leftAlone += done.leftAlone;
        if (done.job !== null) jobs.push(done.job);
        for (const f of temp) delete picked[f.path];
      }
      if (dupGroups.length) {
        const done = await cleanupTrashDuplicates(dupGroups);
        leftAlone += done.leftAlone;
        if (done.job !== null) jobs.push(done.job);
      }
    } catch (e) {
      failure = String(e);
    }
    const notes = [];
    if (failure) notes.push(failure);
    if (leftAlone) {
      notes.push(
        `${leftAlone === 1 ? "1 file was" : `${leftAlone} files were`} left alone: changed since the list was made, the copy kept in a group of duplicates, or not safe to move. Files that do not fit in the Recycle Bin are reported when the move ends.`,
      );
    }
    // Jobs that ended before their ids were known are not waited for.
    trashJobs = jobs.filter((id) => !ended[id]);
    moving = false;
    if (!trashJobs.length) await refreshAll();
    error = notes.join(" ");
    dialog.focus();
  }

  onMount(() => {
    dialog.focus();
    listDrives()
      .then((d) => (drives = d))
      .catch(() => {});
    const unlisten = onQueueUpdate((job) => {
      if (!["done", "failed", "cancelled"].includes(job.state)) return;
      ended[job.id] = true;
      if (!trashJobs.includes(job.id)) return;
      trashJobs = trashJobs.filter((id) => id !== job.id);
      // All moved: every list is out of date, the space has changed.
      if (!trashJobs.length) void refreshAll();
    });
    return () => {
      cleanupDuplicatesCancel().catch(() => {});
      unlisten.then((u) => u());
    };
  });
</script>

<section bind:this={dialog} class="cleanup" aria-label="Cleanup" tabindex="-1">
  <div class="modes" role="group" aria-label="What to look for">
    <button
      class:chosen={mode === "large"}
      aria-pressed={mode === "large"}
      onclick={() => (mode = "large")}
    >
      Large &amp; old files
    </button>
    <button
      class:chosen={mode === "downloads"}
      aria-pressed={mode === "downloads"}
      onclick={() => (mode = "downloads")}
    >
      Old downloads
    </button>
    <button
      class:chosen={mode === "duplicates"}
      aria-pressed={mode === "duplicates"}
      onclick={() => {
        mode = "duplicates";
        included.duplicates = true;
      }}
    >
      Duplicates
    </button>
    <button
      class:chosen={mode === "partials"}
      aria-pressed={mode === "partials"}
      onclick={() => {
        mode = "partials";
        included.partials = true;
      }}
    >
      Unfinished copies
    </button>
    <button
      class:chosen={mode === "temp"}
      aria-pressed={mode === "temp"}
      onclick={() => {
        mode = "temp";
        included.temp = true;
      }}
    >
      Temp files &amp; caches
    </button>
  </div>

  <div class="filters">
    {#if mode === "duplicates"}
      <label>
        Files bigger than
        <select
          value={settings.cleanupDupMinMb}
          onchange={(e) => updateSettings({ cleanupDupMinMb: Number(e.currentTarget.value) })}
        >
          {#each DUPLICATE_SIZES_MB as mb (mb)}
            <option value={mb}>{sizeLabel(mb)}</option>
          {/each}
        </select>
      </label>
    {:else if mode === "partials"}
      <label>
        Left alone for
        <select
          value={settings.cleanupPartialDays}
          onchange={(e) => updateSettings({ cleanupPartialDays: Number(e.currentTarget.value) })}
        >
          {#each PARTIAL_AGES_DAYS as days (days)}
            <option value={days}>{ageLabel(days)}</option>
          {/each}
        </select>
      </label>
    {:else if mode === "temp"}
      <span class="fixed">Only the places listed below are looked in.</span>
    {:else if mode === "downloads"}
      <label>
        Installers and archives not changed for
        <select
          value={settings.cleanupDownloadsDays}
          onchange={(e) => updateSettings({ cleanupDownloadsDays: Number(e.currentTarget.value) })}
        >
          {#each DOWNLOAD_AGES_DAYS as days (days)}
            <option value={days}>{ageLabel(days)}</option>
          {/each}
        </select>
      </label>
    {:else}
      <label>
        Bigger than
        <select
          value={settings.cleanupMinMb}
          onchange={(e) => updateSettings({ cleanupMinMb: Number(e.currentTarget.value) })}
        >
          {#each CLEANUP_SIZES_MB as mb (mb)}
            <option value={mb}>{sizeLabel(mb)}</option>
          {/each}
        </select>
      </label>
      <label>
        Not changed for
        <select
          value={settings.cleanupAgeDays}
          onchange={(e) => updateSettings({ cleanupAgeDays: Number(e.currentTarget.value) })}
        >
          {#each CLEANUP_AGES_DAYS as days (days)}
            <option value={days}>{ageLabel(days)}</option>
          {/each}
        </select>
      </label>
    {/if}
  </div>

  <div class="body">
    <div class="main">
      {#if error}<p class="error" role="alert">{error}</p>{/if}

      {#if mode === "duplicates"}
        {#if loading && !groups.length}
          <EmptyState icon="search" text="Comparing files. This can take a while for big files…" />
        {:else if !scannedAny}
          <EmptyState
            icon="hardDrive"
            text="Nothing has been scanned yet. Scan a drive in Storage first; duplicates are looked for among the scanned files."
          />
        {:else if !groups.length}
          <EmptyState icon="check" text="No identical files found." />
        {:else}
          <p class="summary" role="status">
            {groups.length}
            {groups.length === 1 ? "group" : "groups"} of identical files. The newest copy of each is
            kept (not one in Downloads, Desktop or Temp, if there is another) unless you pick another;
            the others move to the Recycle Bin. At least one copy always stays, and each is compared with
            the copy that stays just before it moves.
            {#if !complete}The search stopped at its limits, so there may be more.{/if}
            {#if unreadable}{unreadable}
              {unreadable === 1 ? "file" : "files"} could not be read and
              {unreadable === 1 ? "was" : "were"} left out.{/if}
          </p>
          <ul class="list groups" aria-label="Groups of identical files">
            {#each groups as g (g.id)}
              {@const kept = keptIn(g)}
              <li class="group" class:left={skipped[g.id]}>
                <div class="ghead">
                  <span class="fname">
                    {g.files.length} copies, {formatSize(g.size)} each: {formatSize(
                      g.size * (g.files.length - 1),
                    )} to gain
                  </span>
                  <label class="check">
                    <input
                      type="checkbox"
                      aria-label="Leave the group with {name(g.files[0].path)} alone"
                      disabled={busy}
                      bind:checked={skipped[g.id]}
                    />
                    Leave this group alone
                  </label>
                </div>
                <div role="radiogroup" aria-label="Which copy of {name(g.files[0].path)} to keep">
                  {#each g.files as f (f.path)}
                    <div class="copy" class:keeping={f.path === kept}>
                      <label class="row">
                        <input
                          type="radio"
                          name="keep-{g.id}"
                          checked={f.path === kept}
                          disabled={skipped[g.id] || busy}
                          aria-label="Keep {name(f.path)} in {parentOf(f.path) ?? ''}"
                          onchange={() => (keep[g.id] = f.path)}
                        />
                        <span class="thumb"
                          ><Thumbnail path={f.path} name={name(f.path)} size={40} /></span
                        >
                        <span class="text">
                          <span class="fname">{name(f.path)}</span>
                          <span class="where">{parentOf(f.path) ?? ""}</span>
                        </span>
                        <span class="muted">{formatDate(f.modified * 1000)}</span>
                        <span class="verdict">
                          {skipped[g.id]
                            ? "left alone"
                            : f.path === kept
                              ? "kept"
                              : "to Recycle Bin"}
                        </span>
                      </label>
                      <button
                        class="show"
                        aria-label="Show {name(f.path)} in folder"
                        onclick={() => {
                          const dir = parentOf(f.path);
                          if (dir) onopen(dir);
                        }}>Show in folder</button
                      >
                    </div>
                  {/each}
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      {:else if loading && !files.length}
        <EmptyState icon="search" text="Looking…" />
      {:else if !scannedAny}
        <EmptyState
          icon="hardDrive"
          text="Nothing has been scanned yet. Scan a drive in Storage first; cleanup suggests files from the scan."
        />
      {:else if !files.length}
        <EmptyState icon="check" text={nothingFound()} />
      {:else}
        {#if mode === "temp"}
          <p class="note">
            These are files programs can make again. A program that is running may be using one, so
            close the programs first; it is better to wait than to guess. Files go to the Recycle
            Bin like anything else.
            {#if !tempComplete}A place was too big to look through whole, so there may be more.{/if}
          </p>
        {:else if mode === "partials"}
          <p class="note">
            A copy leaves one of these behind when it is stopped before it finishes. Check each one
            before you move it: if Folio told you a copy could not be renamed and is complete at a
            name ending in .folio-partial, that one is your copy, not a leftover. Nothing is listed
            while a copy or move is running. A folder is moved whole. Only scanned folders are
            searched, and a single file under 1 MB is not listed.
          </p>
        {:else if mode === "downloads"}
          <p class="note">
            Archives can be the only copy of something. A folder with other files in it (an unpacked
            program) is skipped.
          </p>
        {/if}
        <p class="summary" role="status">
          {files.length === LIMIT ? `The ${LIMIT} biggest` : files.length} suggested, {formatSize(
            totalBytes,
          )} in all. Nothing is selected until you choose it.
        </p>
        <ul class="list" aria-label="Suggested files">
          {#each files as f (f.path)}
            <li>
              <label class="row">
                <input
                  type="checkbox"
                  aria-label="{name(f.path)}, {formatSize(f.size)}"
                  disabled={busy}
                  bind:checked={picked[f.path]}
                />
                <span class="thumb"><Thumbnail path={f.path} name={name(f.path)} size={40} /></span>
                <span class="text">
                  <span class="fname">{name(f.path)}</span>
                  <span class="where">{parentOf(f.path) ?? ""}</span>
                </span>
                {#if "isDir" in f && f.isDir}<span class="muted">folder</span>{/if}
                {#if "place" in f}<span class="muted">{f.place}</span>{/if}
                <span class="muted">{formatDate(f.modified * 1000)}</span>
                <span class="size">{formatSize(f.size)}</span>
              </label>
              <button
                class="show"
                aria-label="Show {name(f.path)} in folder"
                onclick={() => {
                  const dir = parentOf(f.path);
                  if (dir) onopen(dir);
                }}>Show in folder</button
              >
            </li>
          {/each}
        </ul>
      {/if}

      {#if mode === "temp"}
        <div class="places">
          <h4>Where it looks</h4>
          <ul>
            {#each tempPlaces as p (p.path)}
              <li>
                <strong>{p.label}</strong>, files left alone for {ageLabel(p.minAgeDays)}
                <span class="where">{p.path}</span>
              </li>
            {:else}
              <li>Nowhere on this computer has a place on the list.</li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>

    <aside class="panel" aria-label="Summary of what will move">
      <h3>Summary</h3>
      <ul class="cats">
        {#each plan.categories as c (c.id)}
          <li>
            <label>
              <input type="checkbox" disabled={busy} bind:checked={included[c.id]} />
              <span class="catname">{c.label}</span>
              <span class="catsize">
                {c.count
                  ? `${c.count} ${c.count === 1 ? "file" : "files"} · ${formatSize(c.bytes)}`
                  : "nothing picked"}
              </span>
            </label>
          </li>
        {/each}
      </ul>
      <p class="total" role="status">
        {chosenCount
          ? `${chosenCount} ${chosenCount === 1 ? "file" : "files"}, ${formatSize(chosenBytes)} in all`
          : "Nothing to move yet. Pick files in the lists."}
      </p>
      {#if spaceAfter.length}
        <ul class="after" aria-label="Free space once the Recycle Bin is emptied">
          {#each spaceAfter as d (d.letter)}
            <li>
              {d.letter}
              {#if d.free !== null}
                {formatSize(d.free)} free now, up to {formatSize(d.free + d.bytes)} once the Recycle Bin
                is emptied
              {:else}
                up to {formatSize(d.bytes)} more free once the Recycle Bin is emptied
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
      <p class="note">
        Files go to the Recycle Bin; any that do not fit are left alone. The space only comes back
        when you empty it.
      </p>
      <button class="go" disabled={!chosenCount || busy} onclick={trash}>
        {busy
          ? "Moving…"
          : chosenCount
            ? `Move ${chosenCount} ${chosenCount === 1 ? "file" : "files"} (${formatSize(chosenBytes)}) to Recycle Bin`
            : "Move to Recycle Bin"}
      </button>
    </aside>
  </div>
</section>

<style>
  .cleanup {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    padding: var(--space-xl);
    background: var(--bg-window);
    outline: none;
  }

  .filters {
    display: flex;
    align-items: center;
    gap: var(--space-md);
  }

  .modes {
    display: flex;
    gap: var(--space-xs);
    width: fit-content;
    padding: var(--space-2xs);
    border-radius: var(--radius-md);
    background: var(--bg-control);
  }

  .modes button {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .modes button.chosen {
    background: var(--bg-window);
    color: var(--text-primary);
    font-weight: var(--font-weight-medium);
  }

  label {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--text-secondary);
  }

  select {
    padding: var(--space-xs) var(--space-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-window);
    color: var(--text-primary);
  }

  .summary,
  .note {
    margin: 0;
    color: var(--text-secondary);
  }

  .fixed {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .places {
    margin: var(--space-md) 0 0;
    padding: var(--space-md);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    font-size: var(--font-size-caption);
  }

  .places h4 {
    margin: 0 0 var(--space-xs);
    font-size: var(--font-size-body);
  }

  .places ul {
    display: grid;
    gap: var(--space-xs);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .places .where {
    display: block;
    color: var(--text-tertiary);
    overflow-wrap: anywhere;
  }

  .error {
    margin: 0;
    color: var(--status-danger);
  }

  .list {
    flex: 1;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
  }

  /* Rows that are off screen are not laid out or painted. */
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-xs) var(--space-md);
    border-bottom: 1px solid var(--border-default);
    content-visibility: auto;
    contain-intrinsic-size: auto 56px;
  }

  .row {
    flex: 1;
    min-width: 0;
    gap: var(--space-md);
    color: var(--text-primary);
  }

  .thumb {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 40px;
    height: 40px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .fname,
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .where,
  .muted {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .list li.group {
    flex-direction: column;
    align-items: stretch;
    gap: 0;
    padding: var(--space-sm) var(--space-md);
    contain-intrinsic-size: auto 160px;
  }

  .group.left {
    opacity: 0.6;
  }

  .ghead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding-bottom: var(--space-xs);
  }

  .check {
    gap: var(--space-xs);
  }

  .copy {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-2xs) 0;
  }

  .verdict {
    min-width: 96px;
    color: var(--text-secondary);
    text-align: right;
  }

  .copy.keeping .verdict {
    color: var(--accent-default);
    font-weight: var(--font-weight-medium);
  }

  .size {
    min-width: 72px;
    text-align: right;
    font-weight: var(--font-weight-medium);
  }

  .show {
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
    color: var(--accent-default);
  }

  .show:hover {
    background: var(--bg-hover);
  }

  .body {
    display: flex;
    flex: 1;
    gap: var(--space-xl);
    min-height: 0;
  }

  .main {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: var(--space-md);
    min-width: 0;
    min-height: 0;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: 320px;
    flex-shrink: 0;
    padding: var(--space-lg);
    overflow-y: auto;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    background: var(--bg-subtle);
  }

  .panel h3 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: var(--font-weight-semibold);
  }

  .cats,
  .after {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .cats label {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: var(--space-sm);
    align-items: center;
    color: var(--text-primary);
  }

  .catsize {
    grid-column: 2;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .total {
    margin: 0;
    font-weight: var(--font-weight-medium);
  }

  .after {
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
  }

  .go {
    padding: var(--space-sm) var(--space-lg);
    border-radius: var(--radius-md);
    background: var(--accent-default);
    color: var(--text-on-accent);
    font-weight: var(--font-weight-medium);
  }

  .go:disabled {
    background: var(--bg-control);
    color: var(--text-tertiary);
  }
</style>
