<script setup lang="ts">
/**
 * Answers a grill round without drafting a reply by hand: for each question, take the agent's
 * recommendation or type your own answer. The answers go into the composer's draft for you to read
 * and send (nothing sends itself, D23).
 */
import { computed, reactive } from 'vue'
import { useComposerStore } from '../../stores/composer'
import { grillReply, type GrillQuestion } from './grill'

const props = defineProps<{ sessionId: number; questions: GrillQuestion[] }>()
const emit = defineEmits<{ added: [] }>()
const composer = useComposerStore()
const choice = reactive<Record<string, 'accept' | 'custom' | undefined>>({})
const typed = reactive<Record<string, string>>({})

const answers = computed(() => props.questions.map(question => ({
  question,
  accept: choice[question.id] === 'accept',
  text: choice[question.id] === 'custom' ? typed[question.id] ?? '' : '',
})))
const count = computed(() => grillReply(answers.value).split('\n').filter(Boolean).length)
const draftBusy = computed(() => { const e = composer.entry(props.sessionId); return !e.loaded || e.busy || !!e.pending })

function acceptAll(): void { for (const q of props.questions) choice[q.id] = 'accept' }
function addToDraft(): void {
  const reply = grillReply(answers.value)
  if (!reply) return
  const current = composer.entry(props.sessionId).text.trimEnd()
  composer.edit(props.sessionId, current ? `${current}\n\n${reply}` : reply)
  emit('added')
}
</script>

<template>
  <section class="grill" aria-label="Answer the questions">
    <header>
      <h3>{{ questions.length }} {{ questions.length === 1 ? 'question' : 'questions' }} to answer</h3>
      <button class="quiet" @click="acceptAll">Accept every recommendation</button>
    </header>
    <ol>
      <li v-for="q in questions" :key="q.id">
        <p class="q"><strong>{{ q.id }} · {{ q.title }}</strong></p>
        <div class="options" role="radiogroup" :aria-label="`${q.id} answer`">
          <button class="opt" role="radio" :aria-checked="choice[q.id] === 'accept'" :class="{ on: choice[q.id] === 'accept' }" @click="choice[q.id] = 'accept'">
            <span class="tag">Recommended</span>{{ q.recommendation }}
          </button>
          <button v-if="choice[q.id] !== 'custom'" class="opt own" role="radio" aria-checked="false" @click="choice[q.id] = 'custom'">Answer in my own words…</button>
          <textarea v-else v-model="typed[q.id]" rows="2" :aria-label="`${q.id} your answer`" placeholder="Your answer…" autofocus />
        </div>
      </li>
    </ol>
    <footer>
      <span class="hint">Answers are added to your message below so you can read them before sending.</span>
      <button class="primary" :disabled="!count || draftBusy" @click="addToDraft">Add {{ count || '' }} {{ count === 1 ? 'answer' : 'answers' }} to my message</button>
    </footer>
  </section>
</template>

<style scoped>
.grill { padding: 12px 16px; background: var(--surface); border: 1px solid var(--border); border-left: 3px solid var(--status-input); border-radius: 12px; display: grid; gap: 10px; max-height: 46vh; overflow: auto; }
header, footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
h3 { margin: 0; font-size: 12px; letter-spacing: 0.04em; color: var(--status-input); }
header button { font-size: 12px; }
ol { list-style: none; margin: 0; padding: 0; display: grid; gap: 12px; }
.q { margin: 0 0 4px; font-size: 13px; color: var(--text); }
.options { display: grid; gap: 6px; }
.opt { text-align: left; padding: 8px 12px; border-radius: 10px; font-size: 13px; line-height: 1.5; background: var(--surface-raised); border: 1px solid var(--border-strong); color: var(--text-2); }
.opt:hover { border-color: var(--accent-line); }
.opt.on { border-color: var(--accent); background: var(--accent-soft); color: var(--text); }
.opt.own { color: var(--text-muted); }
.tag { display: inline-block; margin-right: 8px; font-size: 10.5px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--accent); }
textarea { width: 100%; padding: 8px 12px; border-radius: 10px; border: 1px solid var(--accent-line); background: var(--surface-raised); color: var(--text); font: inherit; font-size: 13px; line-height: 1.5; resize: vertical; field-sizing: content; min-height: 44px; }
textarea:focus { outline: none; box-shadow: 0 0 0 3px var(--accent-soft); }
.hint { font-size: 12px; color: var(--text-muted); }
</style>
