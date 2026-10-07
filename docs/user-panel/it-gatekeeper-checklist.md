# IT approval checklist (draft)

> Draft for the owner (BIZ-35). What a company's IT or CAE tool owner
> typically asks before approving a desktop engineering tool. Go through
> it with the panel's gatekeeper; mark each line and add their own
> questions. Lines link to the roadmap item that would close the gap.

| # | Question | LightSim 0.3 today | Gap (roadmap) | Gatekeeper's verdict |
|---|---|---|---|---|
| 1 | Is the installer signed by a known publisher? | No: SmartScreen warns | PLT-32 (Windows), PLT-13 (macOS) | |
| 2 | Do antivirus scanners pass it? | Not checked systematically | PLT-32 | |
| 3 | Can it install for all users, silently, without the user being admin? | Per-user installer | PLT-36 | |
| 4 | Is there an MSI or a package for our software portal? | No | PLT-36 | |
| 5 | What does it do on the network? | A local engine on 127.0.0.1 with a token; no outside calls | Write it down: PLT-36, BIZ-12 | |
| 6 | Where does it store files and settings? | The user's app-data folder | PLT-36 | |
| 7 | Can updates be controlled centrally? | No automatic updates | PLT-18 | |
| 8 | Does it run user code? How is that contained? | Script blocks in a separate, limited process (Known issues) | PLT-02 | |
| 9 | Licence terms: may we mirror the installer, evaluate, buy? | EULA 1.0 bans mirrors; no trial | BIZ-30 (EULA 1.1 proposal) | |
| 10 | Third-party components and their licences (SBOM)? | Yes: THIRD-PARTY-NOTICES.txt, sbom.cdx.json | — | |
| 11 | Security contact and response promise? | SECURITY.md | BIZ-13 | |
| 12 | Privacy statement: what data is collected? | None collected; not written down as a statement | BIZ-12 | |
| 13 | Who supports it, and for how long? | One owner; no support promise | BIZ-32 | |
| 14 | How do we buy (order form, supplier registration, invoice)? | Not yet | BIZ-24 | |
| 15 | Supported systems and requirements? | Windows 10/11 x64, Linux x64; no macOS | PLT-30, PLT-13 | |
| 16 | Your company's own questions | | | |
