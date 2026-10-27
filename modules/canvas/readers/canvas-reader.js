// Canvas's reader (module.json `sites[].reader`): runs inside your signed-in Canvas, in a hidden copy
// of the docked site (the base's embed.rs), for schools that turn access tokens off. Canvas's own
// pages (Dashboard, Planner, Grades, Announcements) are drawn from JSON they ask Canvas for with your
// sign-in; this asks for the same JSON the same way, with no token, and hands it to the module's
// backend (src/main.rs, `ingest`), which reads it exactly as it reads the token's answers (`Raw`).
// Nothing leaves your PC.
//
// args: { action: 'read' }, or { action: 'done', kind, id, override_id, done } to tick an item off
// in Canvas's planner the way the Canvas app's checkbox does.
// Resolves to { signedIn: false } when Canvas has signed you out.
async function canvasReader(args) {
  const DAY = 864e5, now = Date.now();
  const iso = t => new Date(t).toISOString().replace(/\.\d+Z$/, 'Z');
  const origin = location.origin;
  const signedOut = () => Object.assign(new Error('Sign in to Canvas in the Command Center first'), { signedOut: true });

  /** One API call with this page's sign-in. Canvas puts `while(1);` before JSON for cookie sign-ins. */
  async function call(url, init = {}) {
    let r;
    try {
      r = await fetch(url.startsWith('/') ? '/api/v1' + url : url, {
        credentials: 'same-origin',
        ...init,
        headers: { Accept: 'application/json', 'X-Requested-With': 'XMLHttpRequest', ...(init.headers || {}) }
      });
    } catch (e) {
      throw new Error('Couldn\'t reach Canvas. Are you online?');
    }
    const text = await r.text();
    let body = null;
    try { body = JSON.parse(text.replace(/^while\(1\);/, '')); } catch (e) {}
    if (r.status === 401) throw signedOut();
    if (!r.ok) {
      const why = body && body.errors && body.errors[0] && body.errors[0].message;
      throw new Error(`Canvas answered HTTP ${r.status}${why ? `: ${why}` : ''}`);
    }
    // The next page of a list, from the Link header (only ever this Canvas's own API).
    const m = /<([^>]+)>;\s*rel="next"/.exec(r.headers.get('Link') || '');
    const next = m && m[1].startsWith(origin + '/api/v1/') ? m[1] : null;
    return { body, next };
  }

  /** Every page of a list, up to `pages` of them. */
  async function list(path, pages = 5) {
    let out = [], url = path;
    for (let i = 0; url && i < pages; i++) {
      const r = await call(url);
      if (Array.isArray(r.body)) out = out.concat(r.body);
      url = r.next;
    }
    return out;
  }

  const pick = (o, keys) => (o && typeof o === 'object' ? Object.fromEntries(keys.filter(k => k in o).map(k => [k, o[k]])) : o);
  const override = o => o && { id: o.id, marked_complete: o.marked_complete };

  if (args && args.action === 'done') {
    const kind = String(args.kind || ''), id = Number(args.id), oid = args.override_id == null ? null : Number(args.override_id);
    if (!/^[a-z_]+$/.test(kind) || !Number.isSafeInteger(id) || (oid !== null && !Number.isSafeInteger(oid))) throw new Error('That isn\'t a Canvas item');
    // Writes need the CSRF token Canvas keeps in a cookie for its own pages.
    const csrf = decodeURIComponent((/(?:^|;\s*)_csrf_token=([^;]*)/.exec(document.cookie) || [])[1] || '');
    const body = oid !== null ? { marked_complete: !!args.done } : { plannable_type: kind, plannable_id: id, marked_complete: !!args.done };
    const r = await call(oid !== null ? `/planner/overrides/${oid}` : '/planner/overrides', {
      method: oid !== null ? 'PUT' : 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
      body: JSON.stringify(body)
    });
    return { id: (r.body && r.body.id) || oid };
  }

  try {
    return await read();
  } catch (e) {
    if (e.signedOut) return { signedIn: false };
    throw e;
  }

  async function read() {
    const user = (await call('/users/self')).body;
    const errors = {};
    /** A part that may fail on its own (the tab shows the rest). */
    const part = (name, p, empty = []) => p.catch(e => { if (e.signedOut) throw e; errors[name] = e.message; return empty; });

    const [courses, colors, planner, missing] = await Promise.all([
      list('/courses?enrollment_state=active&include[]=total_scores&per_page=100'),
      part('colors', call('/users/self/colors').then(r => r.body), null),
      list(`/planner/items?start_date=${iso(now - 14 * DAY)}&end_date=${iso(now + 21 * DAY)}&per_page=100`),
      part('missing', list('/users/self/missing_submissions?include[]=planner_overrides&filter[]=submittable&per_page=100'))
    ]);
    const ids = courses.filter(c => c && typeof c.name === 'string' && Number.isSafeInteger(c.id)).map(c => c.id);
    const [announcements, graded] = await Promise.all([
      ids.length
        ? part('announcements', list(`/announcements?${ids.map(id => `context_codes[]=course_${id}`).join('&')}&start_date=${iso(now - 21 * DAY)}&end_date=${iso(now + DAY)}&per_page=50`, 2))
        : [],
      // What was graded lately, course by course, with the scores (what Canvas's Grades page shows).
      Promise.all(ids.map(id => part('graded', list(`/courses/${id}/students/submissions?workflow_state=graded&graded_since=${iso(now - 21 * DAY)}&include[]=assignment&order=graded_at&order_direction=descending&per_page=20`, 1))
        .then(subs => subs.map(s => ({ ...s, course_id: id }))))).then(a => a.flat())
    ]);

    // Only what the backend reads: descriptions and messages can be long.
    return {
      signedIn: true,
      user: pick(user, ['id', 'name', 'short_name']),
      courses: courses.map(c => ({ ...pick(c, ['id', 'name', 'course_code']), enrollments: (c.enrollments || []).map(e => pick(e, ['type', 'computed_current_score', 'computed_current_grade'])) })),
      colors,
      planner: planner.map(it => ({
        ...pick(it, ['plannable_type', 'plannable_id', 'plannable_date', 'course_id', 'html_url']),
        plannable: pick(it.plannable, ['title', 'name', 'due_at', 'todo_date', 'points_possible', 'assignment_id']),
        submissions: pick(it.submissions, ['submitted', 'graded', 'missing', 'late', 'excused']),
        planner_override: override(it.planner_override)
      })),
      missing: missing.map(a => ({ ...pick(a, ['id', 'name', 'course_id', 'due_at', 'points_possible', 'html_url']), planner_override: override(a.planner_override) })),
      announcements: announcements.map(a => ({ ...pick(a, ['id', 'title', 'context_code', 'posted_at', 'html_url']), message: String(a.message || '').slice(0, 4000) })),
      graded: graded.map(s => ({
        ...pick(s, ['assignment_id', 'course_id', 'score', 'grade', 'graded_at', 'late', 'excused', 'entered_score']),
        assignment: pick(s.assignment, ['id', 'name', 'points_possible', 'html_url', 'course_id'])
      })),
      errors
    };
  }
}
