<script lang="ts">
  import { getButtonStyle } from '$lib/themes';
  import { type RankedSites, Ranking } from '$lib/rankings';
  import { hostRankingsStore } from '$lib/stores';

  let input: HTMLInputElement;

  const rankSite = (site: string, ranking: Ranking) => {
    hostRankingsStore.update(($rankings) => ({
      ...$rankings,
      [site]: ranking,
    }));
  };

  // Called when the user selects an optic file for import
  const importOpticFile = async () => {
    const { default: init, Optic } = await import('client-wasm');
    // Initialize the wasm module
    await init();

    // Get an array of the uploaded files
    let files: File[] = [...(input.files || new FileList())];

    // Iterate through all files, attempt to get the contents & parse the optic
    files.forEach((file) => {
      if (file) {
        const reader = new FileReader();
        reader.readAsText(file, 'UTF-8');

        reader.onload = (readerEvent) => {
          const content = readerEvent.target?.result ?? '';
          try {
            const extractedRankings: RankedSites = JSON.parse(
              Optic.parsePreferenceOptic(content as string),
            );
            // Iterate through all sites in each Ranking and pass them to rankSite
            for (const [_, ranking] of Object.entries(Ranking)) {
              const sites = extractedRankings[ranking];
              sites.forEach((site) => rankSite(site, ranking));
            }
          } catch {
            console.error(
              `Failed to import optic from "${file.name}", please check the formatting.`,
            );
          }
        };
      }
    });
  };
</script>

<input
  bind:this={input}
  type="file"
  accept=".optic"
  id="optic-import"
  multiple
  on:change={importOpticFile}
  hidden
/>
<label for="optic-import" class={getButtonStyle()}> Import from optic </label>
