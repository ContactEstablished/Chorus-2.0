<script setup lang="ts">
import { ref } from 'vue'
import { commands, type PendingReply, type SessionAction, type ActionRequest } from '../../ipc'

const props = defineProps<{ sessionId: number; pending: PendingReply }>()
const emit = defineEmits<{ refresh: []; raw: [] }>()
const answers = ref<string[]>(props.pending.details.kind === 'question' ? props.pending.details.questions.map(() => '') : [])
const message = ref('')
const sending = ref(false)
const sent = ref(false)
const error = ref('')
// Retain the exact request after a transport error: a retry must never become another decision.
const retry = ref<ActionRequest | null>(null)

async function send(action: SessionAction): Promise<void> {
  const request = retry.value ?? { runId: props.pending.runId, requestId: crypto.randomUUID(), action }
  sending.value = true
  error.value = ''
  try {
    const result = await commands.sessionAction(props.sessionId, request)
    if (result.status === 'error') { error.value = result.error; retry.value = null }
    else {
      sent.value = true
      retry.value = null
      if (request.action.kind === 'release') emit('raw')
    }
  } catch {
    retry.value = request
    error.value = 'Reply status is uncertain. Check Raw, or retry this same request to retrieve its receipt.'
  } finally { sending.value = false; emit('refresh') }
}
</script>

<template>
  <section class="reply" :class="pending.details.kind" :aria-label="pending.details.kind === 'approval' ? 'Approval request' : 'Agent questions'">
    <div class="heading">
      <strong>{{ pending.details.kind === 'approval' ? 'Approval needed' : 'Your answer is needed' }}</strong>
      <span v-if="!pending.blockedReason && !sent">{{ pending.remainingSecs }}s to reply here</span>
    </div>
    <template v-if="pending.details.kind === 'approval'">
      <h3>{{ pending.details.tool }}</h3>
      <pre>{{ pending.details.input }}</pre>
      <p>Allow once sends permission to run exactly this request. Deny sends your reason to the agent.</p>
    </template>
    <p v-else>Answer every question. Selecting or inserting text does not send it.</p>
    <p v-if="pending.blockedReason || sent" class="status" role="status">
      {{ pending.blockedReason ?? 'Reply attempted. Waiting for the agent to confirm; Raw may have answered first.' }}
    </p>
    <fieldset :disabled="sending || sent || !!pending.blockedReason || !!retry">
      <template v-if="pending.details.kind === 'question'">
        <div v-for="(q, i) in pending.details.questions" :key="i" class="question">
          <strong>{{ q.question }}</strong>
          <span v-if="q.multiSelect">You may name more than one choice in your answer.</span>
          <span class="choices">
            <button v-for="o in q.options" :key="o.label" type="button" :title="o.description" @click="answers[i] = o.label">{{ o.label }}</button>
          </span>
          <textarea v-model="answers[i]" :aria-label="q.question" rows="2" maxlength="16384" />
        </div>
        <p>Send answer returns the text above for each question. Agent processing time varies.</p>
        <button class="primary" type="button" :disabled="answers.some(a => !a.trim())" @click="send({ kind: 'answer', ask_id: pending.askId, answers: [...answers] })">Send answer</button>
      </template>
      <template v-else>
        <label class="question">Optional denial reason<textarea v-model="message" rows="2" maxlength="16384" /></label>
        <div class="buttons">
          <button class="primary" type="button" @click="send({ kind: 'approval', ask_id: pending.askId, decision: 'allow', message: null })">Allow once</button>
          <button type="button" @click="send({ kind: 'approval', ask_id: pending.askId, decision: 'deny', message: message.trim() || null })">Deny</button>
        </div>
      </template>
      <button class="quiet" type="button" @click="send({ kind: 'release', ask_id: pending.askId })">Answer in Raw</button>
    </fieldset>
    <button v-if="pending.blockedReason || sent" class="quiet" type="button" @click="emit('raw')">Open Raw</button>
    <button v-if="retry" :disabled="sending" type="button" @click="send(retry.action)">Check the same reply</button>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p class="foot">Replies apply to this run only. Expiry returns control to the agent without approving anything.</p>
  </section>
</template>

<style scoped>
.reply { border: 1px solid var(--status-input); border-radius: 14px; padding: 18px; background: var(--surface-raised); }
.reply.approval { border-color: var(--status-approval); }
.heading, .buttons, .choices { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.heading { justify-content: space-between; color: var(--status-input); }
.approval .heading { color: var(--status-approval); }
.heading span, .foot { font-size: 12px; color: var(--text-muted); }
h3 { margin: 12px 0 8px; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 340px; overflow: auto; font: 12px/1.5 var(--font-mono); padding: 12px; background: var(--surface); border-radius: 8px; }
p { color: var(--text-2); font-size: 13px; line-height: 1.5; }
fieldset { border: 0; padding: 0; margin: 0; }
.question { display: flex; flex-direction: column; gap: 8px; margin: 14px 0; font-size: 13px; }
textarea { width: 100%; resize: vertical; }
.quiet { margin-top: 12px; }
.error { color: var(--status-failed); }
.status { font-weight: 600; }
</style>
