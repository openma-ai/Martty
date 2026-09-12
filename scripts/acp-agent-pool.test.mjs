import assert from 'node:assert/strict'
import { createInterface } from 'node:readline'
import test from 'node:test'
import { apply } from '../npm/lib/acp-client.js'

function fixture(name, extra = '') {
  return { command: process.execPath, args: ['-e', `
    const lines = require('node:readline').createInterface({input:process.stdin});
    let next = 0;
    lines.on('line', line => {
      const request = JSON.parse(line);
      const send = message => process.stdout.write(JSON.stringify({jsonrpc:'2.0', ...message})+'\\n');
      const reply = result => send({id:request.id,result});
      ${extra}
      if (request.method === 'initialize') reply({protocolVersion:1,agentInfo:{name:${JSON.stringify(name)}},agentCapabilities:{},authMethods:[]});
      else if (request.method === 'session/new') reply({sessionId:'session-'+(++next)});
      else if (request.method === 'session/prompt') {
        send({method:'session/update',params:{sessionId:request.params.sessionId,update:{sessionUpdate:'agent_message_chunk',content:{type:'text',text:${JSON.stringify(name)}}}}});
        setTimeout(() => reply({stopReason:'end_turn',_meta:{fixture:${JSON.stringify(name)},sessionId:request.params.sessionId}}),20);
      } else if (request.id !== undefined) reply({});
    });
  `] }
}

function client(t, agent = fixture('alpha')) {
  const ctx = {}
  apply(ctx, { agent })
  const service = ctx.acpClient
  const pending = new Map()
  const messages = []
  let sequence = 0
  const lines = createInterface({ input: service.stdout })
  lines.on('line', line => {
    const message = JSON.parse(line)
    messages.push(message)
    if (message.method === undefined && pending.has(message.id)) {
      const resolve = pending.get(message.id)
      pending.delete(message.id)
      resolve(message)
    }
  })
  t.after(() => { service.close(); lines.close() })
  return {
    service, messages,
    request(method, params = {}) {
      const id = ++sequence
      return new Promise(resolve => {
        pending.set(id, resolve)
        service.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n')
      })
    },
  }
}

test('changing the default is configuration only; new sessions retain their own Harness', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize', {protocolVersion:1,clientCapabilities:{}})
  const first = (await c.request('session/new')).result.sessionId
  const alpha = c.service.child
  assert.equal(typeof c.service.setDefaultAgent, 'function')
  c.service.setDefaultAgent(fixture('beta'))
  assert.equal(c.service.child, alpha)
  assert.equal(alpha.killed, false)
  assert.equal((await c.request('session/prompt', {sessionId:first,prompt:[]})).result._meta.fixture, 'alpha')

  const second = (await c.request('session/new')).result.sessionId
  assert.notEqual(first, second, 'two Harnesses can return identical server session ids')
  const third = (await c.request('session/new')).result.sessionId
  assert.notEqual(second, third)
  const replies = await Promise.all([first,second,third].map(sessionId => c.request('session/prompt',{sessionId,prompt:[]})))
  assert.deepEqual(replies.map(reply => reply.result._meta.fixture), ['alpha','beta','beta'])
  assert.deepEqual(replies.map(reply => reply.result._meta.sessionId), ['session-1','session-1','session-2'])
  assert.deepEqual(c.messages.filter(message => message.method === 'session/update').slice(-3).map(message => message.params.sessionId).sort(), [first,second,third].sort())
  assert.equal(alpha.killed, false)
})

test('new session failure does not kill or redirect existing sessions', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize', {protocolVersion:1})
  const first = (await c.request('session/new')).result.sessionId
  assert.equal(typeof c.service.setDefaultAgent, 'function')
  c.service.setDefaultAgent({command:'martty-missing-harness-for-session-test',args:[]})
  assert.ok((await c.request('session/new')).error)
  assert.equal((await c.request('session/prompt',{sessionId:first,prompt:[]})).result._meta.fixture, 'alpha')
  c.service.setDefaultAgent(fixture('alpha'))
  assert.ok((await c.request('session/new')).result.sessionId)
})

test('unknown session ids are rejected instead of reaching the default Harness', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize', {protocolVersion:1})
  await c.request('session/new')
  assert.ok((await c.request('session/prompt',{sessionId:'missing',prompt:[]})).error)
})

test('authentication and its session retry remain on the originating Harness after the default changes', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize', {protocolVersion:1})
  const first = (await c.request('session/new')).result.sessionId
  c.service.setDefaultAgent(fixture('beta', `
    if (request.method === 'initialize') { reply({protocolVersion:1,agentInfo:{name:'beta'},agentCapabilities:{},authMethods:[{id:'login',name:'Sign in'}]}); return; }
    if (request.method === 'session/new' && !globalThis.signedIn) { send({id:request.id,error:{code:-32000,message:'Sign in to beta',data:{reason:'credentials'}}}); return; }
    if (request.method === 'authenticate') { globalThis.signedIn = request.params.methodId === 'login'; reply({}); return; }
  `))
  const failed = await c.request('session/new')
  assert.equal(failed.error.code, -32000)
  assert.equal(failed.error.data.reason, 'credentials')
  const method = failed.error.data.marttyConnection.authMethods[0].id
  assert.notEqual(method, 'login', 'auth method ids must identify their connection')
  c.service.setDefaultAgent(fixture('gamma'))
  assert.deepEqual((await c.request('authenticate', {methodId:method})).result, {})
  const retried = await c.request('session/new', {_meta:{marttyAuthMethod:method}})
  assert.equal(retried.result._meta.marttyConnection.agentInfo.name, 'beta')
  const next = await c.request('session/new')
  assert.equal(next.result._meta.marttyConnection.agentInfo.name, 'gamma')
  assert.equal((await c.request('session/prompt',{sessionId:first,prompt:[]})).result._meta.fixture, 'alpha')
})

test('closing the client settles pending requests and terminates every live Harness', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize',{protocolVersion:1})
  const alpha = c.service.child
  await c.request('session/new')
  c.service.setDefaultAgent(fixture('beta', "if(request.method === 'session/prompt') return;"))
  const second = (await c.request('session/new')).result.sessionId
  c.service.selectSession(second)
  const beta = c.service.child
  const pending = c.request('session/prompt',{sessionId:second,prompt:[]})
  c.service.close()
  assert.match((await pending).error.message, /closed/)
  assert.ok(alpha.killed)
  assert.ok(beta.killed)
})

test('server request ids are isolated and their replies go back to the correct Harness', {timeout:5000}, async t => {
  const callback = `
    if (request.method === 'session/prompt') { globalThis.promptId=request.id; send({id:7,method:'session/request_permission',params:{sessionId:request.params.sessionId,options:[]}}); return; }
    if (request.method === undefined) { send({id:globalThis.promptId,result:{stopReason:'end_turn',_meta:{callback:request.id,answer:request.result.answer}}}); return; }
  `
  const c = client(t, fixture('alpha', callback))
  await c.request('initialize',{protocolVersion:1})
  const first = (await c.request('session/new')).result.sessionId
  c.service.setDefaultAgent(fixture('beta', callback))
  const second = (await c.request('session/new')).result.sessionId
  const pending = [first,second].map(sessionId => c.request('session/prompt',{sessionId,prompt:[]}))
  while (c.messages.filter(message => message.method === 'session/request_permission').length < 2) await new Promise(setImmediate)
  const asks = c.messages.filter(message => message.method === 'session/request_permission')
  assert.notEqual(asks[0].id, asks[1].id)
  for (const ask of asks.reverse()) c.service.stdin.write(JSON.stringify({jsonrpc:'2.0',id:ask.id,result:{answer:ask.params.sessionId}})+'\n')
  const replies = await Promise.all(pending)
  assert.deepEqual(replies.map(reply => reply.result._meta), [{callback:7,answer:first},{callback:7,answer:second}])
})

test('a failed initialization preserves the ACP error and leaves older sessions usable', {timeout:5000}, async t => {
  const c = client(t)
  await c.request('initialize',{protocolVersion:1})
  const first = (await c.request('session/new')).result.sessionId
  c.service.setDefaultAgent(fixture('bad', `if(request.method==='initialize') { send({id:request.id,error:{code:-32001,message:'Unsupported agent',data:{reason:'fixture'}}}); return; }`))
  const failed = await c.request('session/new')
  assert.equal(failed.error.code,-32001)
  assert.equal(failed.error.data.reason,'fixture')
  assert.equal((await c.request('session/prompt',{sessionId:first,prompt:[]})).result._meta.fixture,'alpha')
})

test('process exit reports a bounded stderr tail exactly once', {timeout:5000}, async t => {
  const c = client(t, fixture('failure', `
    if(request.method==='initialize') {
      process.stderr.write('x'.repeat(20000)+'diagnostic-tail\\n', () => process.exit(17));
      return;
    }
  `))
  const failed = await c.request('initialize',{protocolVersion:1})
  assert.match(failed.error.message,/17/)
  assert.match(failed.error.message,/diagnostic-tail/)
  assert.equal(failed.error.message.split('Agent stderr:').length,2)
  assert.ok(Buffer.byteLength(failed.error.message)<8400)
})

test('a namespaced session can be resumed with the same Harness after restarting the client', {timeout:5000}, async t => {
  const beta = fixture('beta', `if(request.method==='session/load') { reply({sessionId:request.params.sessionId,_meta:{raw:request.params.sessionId}}); return; }`)
  const first = client(t)
  await first.request('initialize',{protocolVersion:1})
  await first.request('session/new')
  first.service.setDefaultAgent(beta)
  const savedId = (await first.request('session/new')).result.sessionId
  first.service.close()
  const restarted = client(t, beta)
  await restarted.request('initialize',{protocolVersion:1})
  const loaded = await restarted.request('session/load',{sessionId:savedId})
  assert.equal(loaded.result.sessionId,savedId)
  assert.equal(loaded.result._meta.raw,'session-1')
  assert.equal((await restarted.request('session/prompt',{sessionId:savedId,prompt:[]})).result._meta.fixture,'beta')
})
