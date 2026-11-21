// The providers' own marks, monochrome (`currentColor`, 24 grid), from
// @lobehub/icons-static-svg (MIT), and their avatars as @lobehub/icons
// 5.23.0 (MIT) draws them. Prime Intellect's, which the set lacks, is the
// mark of the logo on its site (primeintellect.ai/icons/primeintellect-logo.svg,
// its two first paths, its words left out). The MCP services' marks come
// from simple-icons (CC0) with each brand's colour there, the few it lacks
// from the same @lobehub set. Logos are not redrawn: the hand-drawn rule
// (icons.tsx) is for our icons; a local server without a mark has ours, a
// screen.
import type { CSSProperties } from "react";
import { IconComputer } from "./icons";
import anthropic from "@lobehub/icons-static-svg/icons/anthropic.svg";
import azure from "@lobehub/icons-static-svg/icons/azure.svg";
import bedrock from "@lobehub/icons-static-svg/icons/bedrock.svg";
import cerebras from "@lobehub/icons-static-svg/icons/cerebras.svg";
import cloudflare from "@lobehub/icons-static-svg/icons/cloudflare.svg";
import deepseek from "@lobehub/icons-static-svg/icons/deepseek.svg";
import fireworks from "@lobehub/icons-static-svg/icons/fireworks.svg";
import gemini from "@lobehub/icons-static-svg/icons/gemini.svg";
import githubcopilot from "@lobehub/icons-static-svg/icons/githubcopilot.svg";
import groq from "@lobehub/icons-static-svg/icons/groq.svg";
import huggingface from "@lobehub/icons-static-svg/icons/huggingface.svg";
import kimi from "@lobehub/icons-static-svg/icons/kimi.svg";
import lmstudio from "@lobehub/icons-static-svg/icons/lmstudio.svg";
import mcp from "@lobehub/icons-static-svg/icons/mcp.svg";
import minimax from "@lobehub/icons-static-svg/icons/minimax.svg";
import mistral from "@lobehub/icons-static-svg/icons/mistral.svg";
import moonshot from "@lobehub/icons-static-svg/icons/moonshot.svg";
import ollama from "@lobehub/icons-static-svg/icons/ollama.svg";
import openai from "@lobehub/icons-static-svg/icons/openai.svg";
import opencode from "@lobehub/icons-static-svg/icons/opencode.svg";
import openrouter from "@lobehub/icons-static-svg/icons/openrouter.svg";
import vercel from "@lobehub/icons-static-svg/icons/vercel.svg";
import vertexai from "@lobehub/icons-static-svg/icons/vertexai.svg";
import vllm from "@lobehub/icons-static-svg/icons/vllm.svg";
import workersai from "@lobehub/icons-static-svg/icons/workersai.svg";
import xai from "@lobehub/icons-static-svg/icons/xai.svg";
import xiaomimimo from "@lobehub/icons-static-svg/icons/xiaomimimo.svg";
import zai from "@lobehub/icons-static-svg/icons/zai.svg";
import primeintellect from "../media/logos/primeintellect.svg";
import azureColor from "@lobehub/icons-static-svg/icons/azure-color.svg";
import geminiColor from "@lobehub/icons-static-svg/icons/gemini-color.svg";
import huggingfaceColor from "@lobehub/icons-static-svg/icons/huggingface-color.svg";
import kimiColor from "@lobehub/icons-static-svg/icons/kimi-color.svg";
import vllmColor from "@lobehub/icons-static-svg/icons/vllm-color.svg";
import awsColor from "@lobehub/icons-static-svg/icons/aws-color.svg";
import greptileColor from "@lobehub/icons-static-svg/icons/greptile-color.svg";
import lovableColor from "@lobehub/icons-static-svg/icons/lovable-color.svg";
import siAirtable from "simple-icons/icons/airtable.svg";
import siAppwrite from "simple-icons/icons/appwrite.svg";
import siAtlassian from "simple-icons/icons/atlassian.svg";
import siClickhouse from "simple-icons/icons/clickhouse.svg";
import siClickup from "simple-icons/icons/clickup.svg";
import siCloudflare from "simple-icons/icons/cloudflare.svg";
import siCloudinary from "simple-icons/icons/cloudinary.svg";
import siDropbox from "simple-icons/icons/dropbox.svg";
import siExpo from "simple-icons/icons/expo.svg";
import siGithub from "simple-icons/icons/github.svg";
import siGrafana from "simple-icons/icons/grafana.svg";
import siHostinger from "simple-icons/icons/hostinger.svg";
import siIntercom from "simple-icons/icons/intercom.svg";
import siLinear from "simple-icons/icons/linear.svg";
import siMapbox from "simple-icons/icons/mapbox.svg";
import siMercadopago from "simple-icons/icons/mercadopago.svg";
import siNeon from "simple-icons/icons/neon.svg";
import siNetlify from "simple-icons/icons/netlify.svg";
import siNewrelic from "simple-icons/icons/newrelic.svg";
import siNotion from "simple-icons/icons/notion.svg";
import siPagerduty from "simple-icons/icons/pagerduty.svg";
import siPosthog from "simple-icons/icons/posthog.svg";
import siPostman from "simple-icons/icons/postman.svg";
import siPrisma from "simple-icons/icons/prisma.svg";
import siPydantic from "simple-icons/icons/pydantic.svg";
import siRailway from "simple-icons/icons/railway.svg";
import siResend from "simple-icons/icons/resend.svg";
import siRevenuecat from "simple-icons/icons/revenuecat.svg";
import siSanity from "simple-icons/icons/sanity.svg";
import siSentry from "simple-icons/icons/sentry.svg";
import siSonatype from "simple-icons/icons/sonatype.svg";
import siStripe from "simple-icons/icons/stripe.svg";
import siWix from "simple-icons/icons/wix.svg";
import siZapier from "simple-icons/icons/zapier.svg";
import siZoho from "simple-icons/icons/zoho.svg";
import siZoom from "simple-icons/icons/zoom.svg";

/** By the agent's provider id; the local servers by the id they get when added (crates/uh-core/src/models/local.rs). */
const LOGOS: Record<string, string> = {
  anthropic,
  "amazon-bedrock": bedrock,
  "azure-openai-responses": azure,
  cerebras,
  "cloudflare-ai-gateway": cloudflare,
  "cloudflare-workers-ai": workersai,
  deepseek,
  fireworks,
  google: gemini,
  "google-antigravity": gemini,
  "google-vertex": vertexai,
  "github-copilot": githubcopilot,
  groq,
  huggingface,
  "kimi-coding": kimi,
  lmstudio,
  minimax,
  "minimax-cn": minimax,
  mistral,
  moonshotai: moonshot,
  "moonshotai-cn": moonshot,
  ollama,
  openai,
  "openai-codex": openai,
  opencode,
  "opencode-go": opencode,
  openrouter,
  "prime-inference": primeintellect,
  "vercel-ai-gateway": vercel,
  vllm,
  xai,
  xiaomi: xiaomimimo,
  "xiaomi-token-plan-ams": xiaomimimo,
  "xiaomi-token-plan-cn": xiaomimimo,
  "xiaomi-token-plan-sgp": xiaomimimo,
  zai,
};

/**
 * A brand's avatar, @lobehub/icons' own (its `style.js` and `Avatar`): its
 * mark in `color` (a colour mark keeps its colours) on `background`. `size`
 * is its share of the disc there (0.75 for most); here the marks keep the
 * plain discs' size and only the brands' differences (styles.css).
 */
type Avatar = { svg: string; background: string; color: string; size: number };

const avatar = (svg: string, background: string, color: string, size: number): Avatar => ({
  svg,
  background,
  color,
  size,
});

const BLACK = "#000";
const WHITE = "#fff";

/** By the agent's provider id, as LOGOS. */
const AVATARS: Record<string, Avatar> = {
  anthropic: avatar(anthropic, "#F1F0E8", "#141413", 0.75),
  "amazon-bedrock": avatar(bedrock, "linear-gradient(45deg, #9AD8F8, #3D8FFF, #6350FB)", WHITE, 0.75),
  "azure-openai-responses": avatar(azureColor, WHITE, WHITE, 0.7),
  cerebras: avatar(cerebras, "#F15A29", WHITE, 0.8),
  "cloudflare-ai-gateway": avatar(cloudflare, "#F38020", WHITE, 0.75),
  "cloudflare-workers-ai": avatar(workersai, "#F38020", WHITE, 0.75),
  deepseek: avatar(deepseek, "#4D6BFE", WHITE, 0.75),
  fireworks: avatar(fireworks, "#5019C5", BLACK, 0.75),
  google: avatar(geminiColor, WHITE, WHITE, 0.8),
  "google-antigravity": avatar(geminiColor, WHITE, WHITE, 0.8),
  "google-vertex": avatar(vertexai, "#4285F4", WHITE, 0.6),
  "github-copilot": avatar(githubcopilot, BLACK, WHITE, 0.75),
  groq: avatar(groq, "#F55036", WHITE, 0.75),
  huggingface: avatar(huggingfaceColor, WHITE, WHITE, 0.75),
  "kimi-coding": avatar(kimiColor, BLACK, WHITE, 0.6),
  lmstudio: avatar(lmstudio, "linear-gradient(135deg, #6C78EF, #4F14BE)", WHITE, 0.7),
  minimax: avatar(minimax, "linear-gradient(to right, #E2167E, #FE603C)", WHITE, 0.75),
  "minimax-cn": avatar(minimax, "linear-gradient(to right, #E2167E, #FE603C)", WHITE, 0.75),
  mistral: avatar(mistral, "#FA520F", WHITE, 0.75),
  moonshotai: avatar(moonshot, "#16191E", WHITE, 0.75),
  "moonshotai-cn": avatar(moonshot, "#16191E", WHITE, 0.75),
  ollama: avatar(ollama, WHITE, BLACK, 0.75),
  openai: avatar(openai, BLACK, WHITE, 0.75),
  "openai-codex": avatar(openai, BLACK, WHITE, 0.75),
  opencode: avatar(opencode, BLACK, WHITE, 0.75),
  "opencode-go": avatar(opencode, BLACK, WHITE, 0.75),
  openrouter: avatar(openrouter, BLACK, "#C8FF00", 0.75),
  // White on black, as Prime Intellect shows it.
  "prime-inference": avatar(primeintellect, BLACK, WHITE, 0.75),
  "vercel-ai-gateway": avatar(vercel, BLACK, WHITE, 0.6),
  vllm: avatar(vllmColor, WHITE, WHITE, 0.6),
  xai: avatar(xai, WHITE, BLACK, 0.65),
  xiaomi: avatar(xiaomimimo, BLACK, WHITE, 0.7),
  "xiaomi-token-plan-ams": avatar(xiaomimimo, BLACK, WHITE, 0.7),
  "xiaomi-token-plan-cn": avatar(xiaomimimo, BLACK, WHITE, 0.7),
  "xiaomi-token-plan-sgp": avatar(xiaomimimo, BLACK, WHITE, 0.7),
  zai: avatar(zai, BLACK, WHITE, 0.6),
};

/** simple-icons draws without a fill: the avatar's colour fills its mark. */
const filled = (svg: string) => svg.replace("<svg ", '<svg fill="currentColor" ');

/** A simple-icons mark on its brand's colour, white or black over it, whichever reads. */
const brand = (svg: string, background: string, color: string): Avatar => avatar(filled(svg), background, color, 0.6);

/** The MCP services by their id in Prime's catalog; the others show their first letter. */
const SERVICE_AVATARS: Record<string, Avatar> = {
  airtable: brand(siAirtable, "#18BFFF", WHITE),
  appwrite: brand(siAppwrite, "#FD366E", WHITE),
  atlassian: brand(siAtlassian, "#0052CC", WHITE),
  "atlassian-rovo": brand(siAtlassian, "#0052CC", WHITE),
  "catalyst-by-zoho": brand(siZoho, "#E42527", WHITE),
  clickhouse: brand(siClickhouse, "#FFCC01", BLACK),
  clickup: brand(siClickup, "#7B68EE", WHITE),
  "cloudflare-api": brand(siCloudflare, "#F38020", WHITE),
  "cloudinary-analysis": brand(siCloudinary, "#3448C5", WHITE),
  "cloudinary-asset-mgmt": brand(siCloudinary, "#3448C5", WHITE),
  "cloudinary-env-config": brand(siCloudinary, "#3448C5", WHITE),
  "cloudinary-smd": brand(siCloudinary, "#3448C5", WHITE),
  dropbox: brand(siDropbox, "#0061FF", WHITE),
  expo: brand(siExpo, "#1C2024", WHITE),
  github: brand(siGithub, "#181717", WHITE),
  "grafana-cloud": brand(siGrafana, "#F46800", WHITE),
  hostinger: brand(siHostinger, "#673DE6", WHITE),
  intercom: brand(siIntercom, "#6AFDEF", BLACK),
  linear: brand(siLinear, "#5E6AD2", WHITE),
  logfire: brand(siPydantic, "#E92063", WHITE),
  mapbox: brand(siMapbox, "#000000", WHITE),
  "mapbox-devkit": brand(siMapbox, "#000000", WHITE),
  mercadopago: brand(siMercadopago, "#00B1EA", WHITE),
  neon: brand(siNeon, "#34D59A", BLACK),
  netlify: brand(siNetlify, "#00C7B7", WHITE),
  newrelic: brand(siNewrelic, "#1CE783", BLACK),
  notion: brand(siNotion, "#000000", WHITE),
  pagerduty: brand(siPagerduty, "#06AC38", WHITE),
  posthog: brand(siPosthog, "#000000", WHITE),
  postman: brand(siPostman, "#FF6C37", WHITE),
  prisma: brand(siPrisma, "#2D3748", WHITE),
  railway: brand(siRailway, "#0B0D0E", WHITE),
  resend: brand(siResend, "#000000", WHITE),
  revenuecat: brand(siRevenuecat, "#F2545B", WHITE),
  sanity: brand(siSanity, "#0D0E12", WHITE),
  sentry: brand(siSentry, "#362D59", WHITE),
  "sonatype-guide": brand(siSonatype, "#1B1C30", WHITE),
  stripe: brand(siStripe, "#635BFF", WHITE),
  wix: brand(siWix, "#0C6EFC", WHITE),
  zapier: brand(siZapier, "#FF4F00", WHITE),
  zoom: brand(siZoom, "#0B5CFF", WHITE),
  "zoom-canvas": brand(siZoom, "#0B5CFF", WHITE),
  "zoom-chat": brand(siZoom, "#0B5CFF", WHITE),
  "zoom-meetings": brand(siZoom, "#0B5CFF", WHITE),
  "zoom-revenue-accelerator": brand(siZoom, "#0B5CFF", WHITE),
  "zoom-tasks": brand(siZoom, "#0B5CFF", WHITE),
  "zoom-whiteboard": brand(siZoom, "#0B5CFF", WHITE),
  "aws-devops-agent": avatar(awsColor, WHITE, "#232F3E", 0.75),
  greptile: avatar(greptileColor, WHITE, WHITE, 0.7),
  lovable: avatar(lovableColor, WHITE, WHITE, 0.7),
};

/** The set's `<title>` would add a tooltip of its own. */
const markup = (svg: string) => svg.replace(/<title>[^<]*<\/title>/, "");

/** The provider's mark, else a local server's screen, else the first letter of its name. */
export function ProviderLogo({ provider, name, local }: { provider: string; name: string; local?: boolean }) {
  const svg = LOGOS[provider];
  if (!svg && local) {
    return (
      <span className="logo" aria-hidden="true">
        <IconComputer />
      </span>
    );
  }
  if (!svg) {
    return (
      <span className="logo is-letter" aria-hidden="true">
        {(name || provider).charAt(0).toUpperCase()}
      </span>
    );
  }
  return <span className="logo" aria-hidden="true" dangerouslySetInnerHTML={{ __html: markup(svg) }} />;
}

/**
 * The provider's avatar, a disc with `className` (its size, its state): the
 * brand's own colours; without an avatar, the plain disc with its mark or
 * letter.
 */
export function ProviderAvatar({
  provider,
  name,
  className,
  local,
}: {
  provider: string;
  name: string;
  className: string;
  local?: boolean;
}) {
  const brand = AVATARS[provider];
  if (!brand) {
    return (
      <span className={className}>
        <ProviderLogo provider={provider} name={name} local={local} />
      </span>
    );
  }
  const style = { background: brand.background, color: brand.color, "--uh-mark": brand.size / 0.75 } as CSSProperties;
  return (
    <span className={`${className} is-brand`} style={style}>
      <span className="logo" aria-hidden="true" dangerouslySetInnerHTML={{ __html: markup(brand.svg) }} />
    </span>
  );
}

/** An MCP service of the catalog, by its id: its avatar, else the plain disc with its first letter. */
export function ServiceAvatar({ service, name, className }: { service: string; name: string; className: string }) {
  const found = SERVICE_AVATARS[service];
  if (!found) {
    return (
      <span className={className}>
        <ProviderLogo provider="" name={name} />
      </span>
    );
  }
  const style = { background: found.background, color: found.color, "--uh-mark": found.size / 0.75 } as CSSProperties;
  return (
    <span className={`${className} is-brand`} style={style}>
      <span className="logo" aria-hidden="true" dangerouslySetInnerHTML={{ __html: markup(found.svg) }} />
    </span>
  );
}

/** A server you added: the MCP mark. */
export function McpLogo() {
  return <span className="logo" aria-hidden="true" dangerouslySetInnerHTML={{ __html: markup(mcp) }} />;
}
