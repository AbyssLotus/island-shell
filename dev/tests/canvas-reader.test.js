// Canvas's reader (modules/canvas/readers/canvas-reader.js) against a stand-in for Canvas's API: what
// it asks for, what it keeps, and what it does when Canvas signs you out. Its answer for the stand-in
// is modules/canvas/src/testdata/read.json, which the backend's own tests read the tab from, so the
// two halves are tested against one file. After changing the reader on purpose, rewrite that file
// with `CANVAS_READ_FIXTURE=write node --test dev/tests/canvas-reader.test.js` and look at the diff.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';

const SRC = readFileSync(new URL('../../modules/canvas/readers/canvas-reader.js', import.meta.url), 'utf8');
const FIXTURE = new URL('../../modules/canvas/src/testdata/read.json', import.meta.url);
const WRITE = process.env.CANVAS_READ_FIXTURE === 'write';
const ORIGIN = 'https://school.example.edu';
// Friday the 24th at 9:41, like the harness.
const NOW = Date.parse('2026-10-24T13:41:00Z');

/** The reader as embed.rs runs it: one function expression, here with the page's globals stood in. */
function reader({ fetch, cookie = '' }) {
  class FixedDate extends Date { static now() { return NOW; } }
  return new Function('fetch', 'location', 'document', 'Date', `return (\n${SRC.trim().replace(/;$/, '')}\n)`)(fetch, { origin: ORIGIN }, { cookie }, FixedDate);
}

const json = (body, { status = 200, link = null, guard = true } = {}) => ({ status, body: (guard ? 'while(1);' : '') + JSON.stringify(body), link });
const long = 'Read chapter 4. '.repeat(400);

/** A Canvas with two courses (and one it won't name), a planner, missing work, an announcement and grades. */
function canvas({ signedIn = true, failCourse2Grades = true } = {}) {
  const calls = [];
  const routes = [
    [/^\/api\/v1\/users\/self$/, () => json({ id: 7, name: 'Sam Student', short_name: 'Sam', avatar_url: 'https://x/a.png', locale: 'en' })],
    [/^\/api\/v1\/courses\?enrollment_state=active&include\[\]=total_scores&per_page=100$/, () => json([
      { id: 1, name: 'Signals and Systems', course_code: 'EE 101', syllabus_body: long, enrollments: [{ type: 'student', computed_current_score: 92.4, computed_current_grade: 'A-', user_id: 7 }] }
    ], { link: `<${ORIGIN}/api/v1/courses?page=2&per_page=100>; rel="next", <${ORIGIN}/api/v1/courses?page=1&per_page=100>; rel="first"` })],
    [/^\/api\/v1\/courses\?page=2&per_page=100$/, () => json([
      { id: 2, name: 'Linear Algebra', course_code: 'MATH 201', enrollments: [{ type: 'student', computed_current_score: 88, computed_current_grade: 'B+' }] },
      { id: 3, access_restricted_by_date: true }
    ])],
    [/^\/api\/v1\/users\/self\/colors$/, () => json({ custom_colors: { course_1: '#4f8cf7', course_2: '#e8704a', user_7: '#000000' } })],
    [/^\/api\/v1\/planner\/items\?start_date=2026-10-10T13:41:00Z&end_date=2026-11-14T13:41:00Z&per_page=100$/, () => json([
      { plannable_type: 'assignment', plannable_id: 11, course_id: 1, plannable_date: '2026-10-25T03:59:00Z', html_url: '/courses/1/assignments/11', context_name: 'Signals and Systems',
        plannable: { id: 11, title: 'Problem set 4', due_at: '2026-10-25T03:59:00Z', points_possible: 20, description: long },
        submissions: { submitted: false, graded: false, missing: false, late: false, excused: false, needs_grading: false, has_feedback: false },
        planner_override: null },
      { plannable_type: 'quiz', plannable_id: 12, course_id: 2, plannable_date: '2026-10-26T15:00:00Z', html_url: '/courses/2/quizzes/12',
        plannable: { id: 12, title: 'Quiz 3', due_at: '2026-10-26T15:00:00Z', points_possible: 10, assignment_id: 112 },
        submissions: { submitted: true, graded: false, missing: false, late: false, excused: false },
        planner_override: { id: 900, plannable_type: 'quiz', plannable_id: 12, marked_complete: true, dismissed: false, user_id: 7 } },
      { plannable_type: 'planner_note', plannable_id: 5, course_id: null, plannable_date: '2026-10-27T16:00:00Z', html_url: '/api/v1/planner_notes/5',
        plannable: { id: 5, title: 'Study group', todo_date: '2026-10-27T16:00:00Z', details: 'Library' },
        submissions: false, planner_override: null }
    ])],
    [/^\/api\/v1\/users\/self\/missing_submissions\?include\[\]=planner_overrides&filter\[\]=submittable&per_page=100$/, () => json([
      { id: 9, name: 'Lab 4: Sampling', course_id: 1, due_at: '2026-10-17T03:59:00Z', points_possible: 30, html_url: `${ORIGIN}/courses/1/assignments/9`, description: long, rubric: [{ points: 30 }], planner_override: null }
    ])],
    [/^\/api\/v1\/announcements\?context_codes\[\]=course_1&context_codes\[\]=course_2&start_date=2026-10-03T13:41:00Z&end_date=2026-10-25T13:41:00Z&per_page=50$/, () => json([
      { id: 5, title: 'Midterm review session moved', context_code: 'course_2', posted_at: '2026-10-22T18:00:00Z', html_url: `${ORIGIN}/courses/2/discussion_topics/5`, message: '<p>The review session is now in room 104 at <b>4 pm</b>.</p>', author: { display_name: 'Prof' } }
    ])],
    [/^\/api\/v1\/courses\/1\/students\/submissions\?workflow_state=graded&graded_since=2026-10-03T13:41:00Z&include\[\]=assignment&order=graded_at&order_direction=descending&per_page=20$/, () => json([
      { id: 501, assignment_id: 8, user_id: 7, score: 27, grade: '27', entered_score: 27, graded_at: '2026-10-23T20:15:00Z', late: false, excused: false, workflow_state: 'graded',
        assignment: { id: 8, name: 'Lab 3: Convolution', points_possible: 30, html_url: `${ORIGIN}/courses/1/assignments/8`, course_id: 1, description: long } },
      // Graded but not posted yet: Canvas leaves the score out.
      { id: 502, assignment_id: 10, user_id: 7, score: null, grade: null, graded_at: '2026-10-24T09:00:00Z', late: false, excused: false, workflow_state: 'graded',
        assignment: { id: 10, name: 'Problem set 3', points_possible: 20, html_url: `${ORIGIN}/courses/1/assignments/10`, course_id: 1 } }
    ])],
    [/^\/api\/v1\/courses\/2\/students\/submissions\?/, () => failCourse2Grades ? json({ errors: [{ message: 'An error occurred.' }] }, { status: 500, guard: false }) : json([])]
  ];
  async function fetch(url, init = {}) {
    const path = url.startsWith(ORIGIN) ? url.slice(ORIGIN.length) : url;
    calls.push({ path, method: init.method || 'GET', headers: init.headers || {}, body: init.body, credentials: init.credentials });
    if (!signedIn) return response(json({ status: 'unauthenticated', errors: [{ message: 'user authorization required' }] }, { status: 401, guard: false }));
    if (/^\/api\/v1\/planner\/overrides/.test(path)) return response(json({ id: path.endsWith('overrides') ? 901 : 900, marked_complete: JSON.parse(init.body).marked_complete }));
    const route = routes.find(([re]) => re.test(path));
    return response(route ? route[1]() : json({ errors: [{ message: 'The specified resource does not exist.' }] }, { status: 404, guard: false }));
  }
  return { fetch, calls };
}

function response({ status, body, link }) {
  return { status, ok: status >= 200 && status < 300, text: async () => body, headers: { get: h => (h === 'Link' ? link : null) } };
}

test('reads what Canvas\'s own pages read, and keeps only what the backend uses', async () => {
  const c = canvas();
  const out = await reader({ fetch: c.fetch })({ action: 'read' });
  if (WRITE) writeFileSync(FIXTURE, JSON.stringify(out, null, 1) + '\n');
  assert.deepEqual(out, JSON.parse(readFileSync(FIXTURE, 'utf8')));
  // Every call carries the page's sign-in and nothing else; no token anywhere.
  assert.ok(c.calls.every(x => x.credentials === 'same-origin' && x.method === 'GET' && !('Authorization' in x.headers)));
  // The second page of courses came from the Link header.
  assert.ok(c.calls.some(x => x.path === '/api/v1/courses?page=2&per_page=100'));
  // Long text stays behind: descriptions aren't kept, announcements are cut short.
  assert.ok(JSON.stringify(out).length < 6000);
});

test('signed out, it says so instead of failing', async () => {
  const out = await reader({ fetch: canvas({ signedIn: false }).fetch })({ action: 'read' });
  assert.deepEqual(out, { signedIn: false });
});

test('a Link header pointing anywhere else is not followed', async () => {
  const c = canvas();
  const real = c.fetch;
  const fetch = async (url, init) => {
    const r = await real(url, init);
    if (url.startsWith('/api/v1/courses?enrollment_state')) return { ...r, headers: { get: () => '<https://evil.example/api/v1/x>; rel="next"' } };
    return r;
  };
  await reader({ fetch })({ action: 'read' });
  assert.ok(!c.calls.some(x => x.path.includes('evil')));
});

test('offline, the error says so', async () => {
  const fetch = async () => { throw new TypeError('Failed to fetch'); };
  await assert.rejects(reader({ fetch })({ action: 'read' }), /Are you online/);
});

test('ticking off goes to Canvas\'s planner with its CSRF token', async () => {
  const c = canvas();
  const read = reader({ fetch: c.fetch, cookie: 'log_session_id=1; _csrf_token=abc%2B%2F1%3D; other=2' });
  assert.deepEqual(await read({ action: 'done', kind: 'assignment', id: 11, override_id: null, done: true }), { id: 901 });
  assert.deepEqual(await read({ action: 'done', kind: 'quiz', id: 12, override_id: 900, done: false }), { id: 900 });
  const [post, put] = c.calls;
  assert.deepEqual([post.method, post.path, post.headers['X-CSRF-Token']], ['POST', '/api/v1/planner/overrides', 'abc+/1=']);
  assert.deepEqual(JSON.parse(post.body), { plannable_type: 'assignment', plannable_id: 11, marked_complete: true });
  assert.deepEqual([put.method, put.path, JSON.parse(put.body)], ['PUT', '/api/v1/planner/overrides/900', { marked_complete: false }]);
  await assert.rejects(read({ action: 'done', kind: 'x/../y', id: 1, done: true }), /isn't a Canvas item/);
  await assert.rejects(read({ action: 'done', kind: 'assignment', id: '1; drop', done: true }), /isn't a Canvas item/);
});
