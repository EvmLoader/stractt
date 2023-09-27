type Method = 'DELETE' | 'GET' | 'PUT' | 'POST' | 'HEAD' | 'TRACE' | 'PATCH';

let GLOBAL_API_BASE = '';
export const getApiBase = (options?: ApiOptions) => options?.apiBase ?? GLOBAL_API_BASE;
export const setGlobalApiBase = (apiBase: string) => (GLOBAL_API_BASE = apiBase);

export type ApiOptions = {
  fetch?: typeof fetch;
  apiBase?: string;
};

export const requestPlain = (
  method: Method,
  url: string,
  body?: unknown,
  options?: ApiOptions & { headers?: Record<string, string> },
): {
  data: Promise<string>;
  cancel: (reason?: string) => void;
} => {
  let inFlight = true;
  const controller = new AbortController();
  const data = (options?.fetch ?? fetch)(`${getApiBase(options)}${url}`, {
    method: method.toUpperCase(),
    body: typeof body != 'undefined' ? JSON.stringify(body) : void 0,
    signal: controller.signal,
    headers: options?.headers,
  }).then(async (res) => {
    inFlight = false;
    if (res.ok) {
      const text = await res.text();
      try {
        return text;
      } catch (_) {
        throw text;
      }
    } else {
      throw res.text();
    }
  });

  return {
    data,
    cancel: (reason) => {
      if (inFlight) controller.abort(reason);
    },
  };
};

export const requestJson = <T>(
  method: Method,
  url: string,
  body?: unknown,
  options: ApiOptions = {},
): {
  data: Promise<T>;
  cancel: (reason?: string) => void;
} => {
  const { data, cancel } = requestPlain(method, url, body, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
    },
  });
  return { data: data.then((text) => JSON.parse(text) as T), cancel };
};

export type SSEStream<T> = (
  event:
    | { type: 'message'; data: T }
    | {
        type: 'error';
        event: Event;
      },
) => void;

const sse = <T>(
  _method: Method,
  url: string,
  options?: ApiOptions,
): {
  cancel: () => void;
  listen: (stream: SSEStream<T>) => void;
} => {
  const source = new EventSource(`${getApiBase(options)}${url}`);

  let stream: SSEStream<T> | null = null;

  source.onmessage = (event) => {
    const data = event.data;
    stream?.({ type: 'message', data });
  };
  source.onerror = (event) => {
    stream?.({ type: 'error', event });
  };
  return {
    cancel: () => source.close(),
    listen: (newStream) => (stream = newStream),
  };
};

export const api = {
  alice: (
    query: {
      message: string;
      optic: string;
      prevState: EncodedSavedState;
    },
    options?: ApiOptions,
  ) => sse<ExecutionState>('GET', `alice?${new URLSearchParams(query)}`, options),
  autosuggest: (
    params: {
      q: string;
    },
    options?: ApiOptions,
  ) => requestJson<Suggestion[]>('POST', `autosuggest?${new URLSearchParams(params)}`, options),
  exploreExport: (body: ExploreExportOpticParams, options?: ApiOptions) =>
    requestPlain('POST', `explore/export`, body, options),
  factCheck: (body: FactCheckParams, options?: ApiOptions) =>
    requestJson<FactCheckResponse>('POST', `fact_check`, body, options),
  search: (body: ApiSearchQuery, options?: ApiOptions) =>
    requestJson<ApiSearchResult>('POST', `search`, body, options),
  sitesExport: (body: SitesExportOpticParams, options?: ApiOptions) =>
    requestPlain('POST', `sites/export`, body, options),
  summarize: (
    query: {
      query: string;
      url: string;
    },
    options?: ApiOptions,
  ) => sse<string>('GET', `summarize?${new URLSearchParams(query)}`, options),
  webgraphHostIngoing: (
    query: {
      site: string;
    },
    options?: ApiOptions,
  ) =>
    requestJson<FullEdge[]>('POST', `webgraph/host/ingoing?${new URLSearchParams(query)}`, options),
  webgraphHostKnows: (
    query: {
      site: string;
    },
    options?: ApiOptions,
  ) => requestJson<KnowsSite>('POST', `webgraph/host/knows?${new URLSearchParams(query)}`, options),
  webgraphHostOutgoing: (
    query: {
      site: string;
    },
    options?: ApiOptions,
  ) =>
    requestJson<FullEdge[]>(
      'POST',
      `webgraph/host/outgoing?${new URLSearchParams(query)}`,
      options,
    ),
  webgraphHostSimilar: (body: SimilarSitesParams, options?: ApiOptions) =>
    requestJson<ScoredSite[]>('POST', `webgraph/host/similar`, body, options),
  webgraphPageIngoing: (
    query: {
      page: string;
    },
    options?: ApiOptions,
  ) =>
    requestJson<FullEdge[]>('POST', `webgraph/page/ingoing?${new URLSearchParams(query)}`, options),
  webgraphPageOutgoing: (
    query: {
      page: string;
    },
    options?: ApiOptions,
  ) =>
    requestJson<FullEdge[]>(
      'POST',
      `webgraph/page/outgoing?${new URLSearchParams(query)}`,
      options,
    ),
};

export type ApiSearchQuery = {
  fetchDiscussions?: boolean;
  flattenResponse?: boolean;
  numResults?: number;
  optic?: string;
  page?: number;
  query: string;
  returnRankingSignals?: boolean;
  safeSearch?: boolean;
  selectedRegion?: Region;
  siteRankings?: SiteRankings;
};
export type ApiSearchResult =
  | (WebsitesResult & {
      type: 'websites';
    })
  | (BangHit & {
      type: 'bang';
    });
export type Bang = {
  c?: string;
  d?: string;
  r?: number;
  s?: string;
  sc?: string;
  t: string;
  u: string;
};
export type BangHit = {
  bang: Bang;
  redirectTo: UrlWrapper;
};
export type Calculation = {
  expr: Expr;
  input: string;
  result: number;
};
export type CodeOrText =
  | {
      type: 'code';
      value: string;
    }
  | {
      type: 'text';
      value: string;
    };
export type DisplayedAnswer = {
  answer: string;
  prettyUrl: string;
  snippet: string;
  title: string;
  url: string;
};
export type DisplayedEntity = {
  imageBase64?: string;
  info: string & EntitySnippet[][];
  matchScore: number;
  relatedEntities: DisplayedEntity[];
  smallAbstract: EntitySnippet;
  title: string;
};
export type DisplayedSidebar =
  | {
      type: 'entity';
      value: DisplayedEntity;
    }
  | {
      type: 'stackOverflow';
      value: {
        answer: StackOverflowAnswer;
        title: string;
      };
    };
export type DisplayedWebpage = {
  domain: string;
  prettyUrl: string;
  rankingSignals?: {};
  site: string;
  snippet: Snippet;
  title: string;
  url: string;
};
export type EncodedEncryptedState = string;
export type EncodedSavedState = string;
export type EntitySnippet = {
  fragments: EntitySnippetFragment[];
};
export type EntitySnippetFragment =
  | {
      kind: 'normal';
      text: string;
    }
  | {
      href: string;
      kind: 'link';
      text: string;
    };
export type ExecutionState =
  | {
      query: string;
      type: 'beginSearch';
    }
  | {
      query: string;
      result: SimplifiedWebsite[];
      type: 'searchResult';
    }
  | {
      text: string;
      type: 'speaking';
    }
  | {
      state: EncodedEncryptedState;
      type: 'done';
    };
export type ExploreExportOpticParams = {
  chosenSites: string[];
  similarSites: string[];
};
export type Expr =
  | {
      Number: number;
    }
  | {
      Op: [{}, {}, {}];
    };
export type FactCheckParams = {
  claim: string;
  evidence: string;
};
export type FactCheckResponse = {
  score: number;
};
export type FullEdge = {
  from: Node;
  label: string;
  to: Node;
};
export type HighlightedSpellCorrection = {
  highlighted: string;
  raw: string;
};
export type KnowsSite =
  | {
      site: string;
      type: 'known';
    }
  | {
      type: 'unknown';
    };
export type Node = {
  name: string;
};
export type Region = 'All' | 'Denmark' | 'France' | 'Germany' | 'Spain' | 'US';
export const REGIONS = ['All', 'Denmark', 'France', 'Germany', 'Spain', 'US'] satisfies Region[];
export type ScoredSite = {
  description?: string;
  score: number;
  site: string;
};
export type SignalScore = {
  coefficient: number;
  value: number;
};
export type SimilarSitesParams = {
  sites: string[];
  topN: number;
};
export type SimplifiedWebsite = {
  site: string;
  text: string;
  title: string;
  url: string;
};
export type SiteRankings = {
  blocked: string[];
  disliked: string[];
  liked: string[];
};
export type SitesExportOpticParams = {
  siteRankings: SiteRankings;
};
export type Snippet =
  | {
      date?: string;
      text: TextSnippet;
      type: 'normal';
    }
  | {
      answers: StackOverflowAnswer[];
      question: StackOverflowQuestion;
      type: 'stackOverflowQA';
    };
export type StackOverflowAnswer = {
  accepted: boolean;
  body: CodeOrText[];
  date: string;
  upvotes: number;
  url: string;
};
export type StackOverflowQuestion = {
  body: CodeOrText[];
};
export type Suggestion = {
  highlighted: string;
  raw: string;
};
export type TextSnippet = {
  fragments: TextSnippetFragment[];
};
export type TextSnippetFragment = {
  kind: TextSnippetFragmentKind;
  text: string;
};
export type TextSnippetFragmentKind = 'normal' | 'highlighted';
export const TEXT_SNIPPET_FRAGMENT_KINDS = [
  'normal',
  'highlighted',
] satisfies TextSnippetFragmentKind[];
export type UrlWrapper = string;
export type WebsitesResult = {
  directAnswer?: DisplayedAnswer;
  discussions?: DisplayedWebpage[];
  hasMoreResults: boolean;
  numHits: number;
  searchDurationMs: number;
  sidebar?: DisplayedSidebar;
  spellCorrectedQuery?: HighlightedSpellCorrection;
  webpages: DisplayedWebpage[];
  widget?: Widget;
};
export type Widget = {
  type: 'calculator';
  value: Calculation;
};
