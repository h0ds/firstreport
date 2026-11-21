// Runs only when a request matches no file in the live site. Story pages
// leave the live site after the build window; their last copy is in the
// archive bucket, so a URL that worked keeps working (docs/deploy.md 4.1).

const TYPES = {
  html: "text/html; charset=utf-8",
  json: "application/json; charset=utf-8",
  md: "text/markdown; charset=utf-8",
};

// "/story/123/" -> "story/123/index.html"; null when it is not a story file.
export function archiveKey(pathname) {
  const m = /^\/story\/(\d+)\/(index\.(html|json|md))?$/.exec(pathname);
  return m ? `story/${m[1]}/${m[2] ?? "index.html"}` : null;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "GET" || request.method === "HEAD") {
      const bare = /^\/story\/(\d+)$/.exec(url.pathname);
      if (bare) {
        url.pathname += "/";
        return Response.redirect(url.toString(), 308);
      }
      const key = archiveKey(url.pathname);
      const object = key && (await env.ARCHIVE.get(key));
      if (object) {
        return new Response(request.method === "HEAD" ? null : object.body, {
          headers: {
            "Content-Type": TYPES[key.split(".").pop()],
            // An archived page never changes again.
            "Cache-Control": "public, max-age=86400, s-maxage=2592000",
            "ETag": object.httpEtag,
            "X-Content-Type-Options": "nosniff",
            "Referrer-Policy": "strict-origin-when-cross-origin",
          },
        });
      }
    }
    // Not archived either: the site's own 404 page.
    return env.ASSETS.fetch(request);
  },
};
