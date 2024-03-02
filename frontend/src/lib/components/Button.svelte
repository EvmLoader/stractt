<script lang="ts">
  import { getButtonStyle, type ButtonKind } from '$lib/themes';

  export let title: string | undefined = void 0;
  export let form: string | undefined = void 0;
  export let type: 'button' | 'submit' | 'reset' | null | undefined = void 0;

  export let pale: boolean | undefined = void 0;
  export let padding: boolean | undefined = void 0;
  export let kind: ButtonKind | undefined = void 0;
  export let _class: string | undefined = void 0;
  export let aria_label: string | undefined = void 0;
</script>

<button
  on:click
  {title}
  {form}
  {type}
  class={getButtonStyle({ padding, pale, kind, _class })}
  aria-label={aria_label}
>
  <slot />
</button>
