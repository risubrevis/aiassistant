import { writable } from "svelte/store";
import { skillsList, onSkillsReloaded, type Skill } from "$lib/tauri";

export const skills = writable<Skill[]>([]);

let inited = false;
export async function loadSkills() {
  try {
    skills.set(await skillsList());
  } catch (e) {
    console.error("loadSkills failed", e);
  }
}

export function initSkillsStore() {
  if (inited) return;
  inited = true;
  void loadSkills();
  onSkillsReloaded(() => void loadSkills());
}