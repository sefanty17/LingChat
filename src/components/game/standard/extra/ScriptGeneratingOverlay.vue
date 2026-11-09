<template>
  <!-- 章节末尾 AI 续写下一章：全屏遮罩拦住误点，等待结束后由 store 收起 -->
  <div v-if="gameStore.scriptGenerating.on" class="script-generating-mask">
    <div class="script-generating-card">
      <div class="script-generating-title">{{ t("game.scriptGenerating.title") }}</div>
      <div v-if="gameStore.scriptGenerating.seq > 0" class="script-generating-chapter">
        {{ t("game.scriptGenerating.stage", { n: gameStore.scriptGenerating.seq }) }}
      </div>
      <div class="script-generating-dots" aria-hidden="true">
        <span></span>
        <span></span>
        <span></span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useGameStore } from "@/stores/modules/game";

const { t } = useI18n();
const gameStore = useGameStore();
</script>

<style scoped>
.script-generating-mask {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  /* 生成期间吞掉所有点击，避免误触对话与选项 */
  pointer-events: auto;
  background: rgba(20, 18, 15, 0.55);
  font-family: var(--font-app), var(--font-sans);
}

.script-generating-card {
  min-width: 240px;
  max-width: 82vw;
  padding: 28px 36px 24px;
  border-radius: 16px;
  background: #ffffff;
  box-shadow:
    0 18px 48px rgba(20, 18, 15, 0.22),
    0 2px 8px rgba(20, 18, 15, 0.08);
  text-align: center;
}

.script-generating-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: #2b2620;
}

.script-generating-chapter {
  margin-top: 8px;
  font-size: 13px;
  line-height: 1.5;
  color: #8a837a;
  word-break: break-all;
}

.script-generating-dots {
  display: flex;
  justify-content: center;
  gap: 6px;
  margin-top: 16px;
}

.script-generating-dots span {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #14614f;
  opacity: 0.3;
  animation: script-generating-bounce 1.2s ease-in-out infinite;
}

.script-generating-dots span:nth-child(2) {
  animation-delay: 0.15s;
}

.script-generating-dots span:nth-child(3) {
  animation-delay: 0.3s;
}

@keyframes script-generating-bounce {
  0%,
  80%,
  100% {
    transform: translateY(0);
    opacity: 0.3;
  }
  40% {
    transform: translateY(-6px);
    opacity: 1;
  }
}
</style>
