import { shouldShowCaptcha } from '$lib/captcha/rateLimiter';
import { extractSearchParams, type SearchParams } from '$lib/search';
import { redirect } from '@sveltejs/kit';
import type { Actions, PageServerLoadEvent } from './$types';

export const load = async ({ locals, getClientAddress, url }: PageServerLoadEvent) => {
  const searchParams: SearchParams | undefined =
    (locals['form'] && extractSearchParams(locals['form'])) || undefined;

  if (await shouldShowCaptcha(getClientAddress())) {
    return redirect(302, `/sorry?redirectTo=${encodeURIComponent(url.toString())}`);
  }

  return { form: searchParams, clientAddress: getClientAddress() };
};

export const actions: Actions = {
  default: async (event) => {
    const { request } = event;

    event.locals.form = await request.formData();

    return { success: true };
  },
} satisfies Actions;
