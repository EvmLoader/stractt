<script lang="ts">
  import { getButtonStyle } from '$lib/themes';
  import { Ranking } from '$lib/rankings';
  import { hostRankingsStore } from '$lib/stores';

  let input: HTMLInputElement;

  const convertRanking: Record<import('client-wasm').Ranking, Ranking> = {
    liked: Ranking.LIKED,
    disliked: Ranking.DISLIKED,
    blocked: Ranking.BLOCKED,
  };

  // Called when the user selects an optic file for import
  const importOpticFile = async () => {
    const { default: init, parsePreferenceOptic } = await import('client-wasm');
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
            const siteRankings = parsePreferenceOptic(content as string);
            const sites = [...siteRankings.sites.entries()];
            hostRankingsStore.update(($rankings) => ({
              ...$rankings,
              ...Object.fromEntries(
                sites.map(([site, ranking]) => [site, convertRanking[ranking]]),
              ),
            }));
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
