"use strict";

/**
 * Signs the Windows build (PLT-32). electron-builder calls this for every file
 * it signs: LightSim.exe, the frozen engine (resources/backend/
 * lightsim-backend.exe), every .dll and .pyd the engine and Electron ship
 * (win.signExts), the installer, its uninstaller and the MSI.
 *
 * Signing needs a certificate the owner pays for, so it only happens when the
 * build has one. With none set, this logs once and leaves the file unsigned:
 * local and pull-request builds still work. Two routes, picked by what is set
 * (all of a route's settings, secrets included; route() is also what the
 * workflow's "Signing available?" step asks, so the two always agree):
 *
 *   Azure Artifact Signing (formerly Trusted Signing)
 *     AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET   an app registration
 *       allowed to sign with the profile below
 *     LIGHTSIM_AZURE_ENDPOINT   e.g. https://weu.codesigning.azure.net
 *     LIGHTSIM_AZURE_ACCOUNT    the Artifact Signing account name
 *     LIGHTSIM_AZURE_PROFILE    its certificate profile name
 *
 *   Any other signing tool, such as an OV certificate in a cloud HSM
 *     LIGHTSIM_SIGN_COMMAND     a command with {file} where the file goes, e.g.
 *       signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /sha1 <thumbprint> "{file}"
 *
 * A .dll or .pyd that is already validly signed (Microsoft's runtime, the
 * Python Software Foundation's) keeps its own signature; everything else,
 * and every .exe and .msi, is signed with LightSim's.
 */

const { execFileSync, execSync } = require("node:child_process");
const path = require("node:path");

let warned = false;
let azureReady = false;

/** What Azure Artifact Signing needs: the app registration (secrets) and
 *  the account and profile (repository variables). */
const AZURE = [
  "AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET",
  "LIGHTSIM_AZURE_ENDPOINT", "LIGHTSIM_AZURE_ACCOUNT", "LIGHTSIM_AZURE_PROFILE",
];

/** "azure", "command" or null (unsigned). A route is taken only when all of
 *  its settings are there: the variables alone, without the secrets, would
 *  start a signing that cannot sign and fail every Windows build. */
function route(env = process.env) {
  if (AZURE.every((key) => env[key])) return "azure";
  if (env.LIGHTSIM_SIGN_COMMAND) return "command";
  return null;
}

/** The Azure settings that are missing when only some are set (names only). */
function missingAzure(env = process.env) {
  const missing = AZURE.filter((key) => !env[key]);
  return missing.length === AZURE.length ? [] : missing;
}

const psQuote = (s) => `'${String(s).replace(/'/g, "''")}'`;

function powershell(command) {
  return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  }).trim();
}

/** Whether the file already carries a valid Authenticode signature. */
function validlySigned(file) {
  try {
    return powershell(`(Get-AuthenticodeSignature -LiteralPath ${psQuote(file)}).Status`) === "Valid";
  } catch {
    return false;
  }
}

function signAzure(file, env) {
  if (!azureReady) {
    powershell(
      "if (-not (Get-Module -ListAvailable -Name TrustedSigning)) {" +
        " Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser | Out-Null;" +
        " Install-Module -Name TrustedSigning -MinimumVersion 0.5.0 -Force -Repository PSGallery -Scope CurrentUser }",
    );
    azureReady = true;
  }
  powershell(
    "Invoke-TrustedSigning" +
      ` -Endpoint ${psQuote(env.LIGHTSIM_AZURE_ENDPOINT)}` +
      ` -CodeSigningAccountName ${psQuote(env.LIGHTSIM_AZURE_ACCOUNT)}` +
      ` -CertificateProfileName ${psQuote(env.LIGHTSIM_AZURE_PROFILE)}` +
      " -FileDigest SHA256 -TimestampRfc3161 'http://timestamp.acs.microsoft.com' -TimestampDigest SHA256" +
      ` -Files ${psQuote(file)}`,
  );
}

exports.default = async function sign(configuration) {
  const file = configuration.path;
  const which = route();
  if (!which) {
    if (!warned) {
      console.log("  • no signing certificate is set: building UNSIGNED (see desktop/build/sign-windows.cjs)");
      const missing = missingAzure();
      if (missing.length) console.log(`  • Azure Artifact Signing is only partly set up; missing: ${missing.join(", ")}`);
      warned = true;
    }
    return;
  }
  // electron-builder asks once per hash; one SHA-256 signature is enough
  // (Windows 7's SHA-1 is not supported by LightSim).
  if (configuration.hash && configuration.hash !== "sha256") return;
  const ext = path.extname(file).toLowerCase();
  if (ext !== ".exe" && ext !== ".msi" && validlySigned(file)) {
    console.log(`  • keeping the existing signature of ${path.basename(file)}`);
    return;
  }
  console.log(`  • signing ${path.basename(file)} (${which})`);
  if (which === "azure") signAzure(file, process.env);
  else execSync(process.env.LIGHTSIM_SIGN_COMMAND.split("{file}").join(file), { stdio: "inherit" });
};

exports.route = route;
exports.missingAzure = missingAzure;

// `node build/sign-windows.cjs --route` prints the route this environment
// signs with ("none" without one), for the workflow's "Signing available?"
// step.
if (require.main === module && process.argv.includes("--route")) {
  process.stdout.write(`${route() ?? "none"}\n`);
}
