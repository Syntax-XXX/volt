#!/usr/bin/env python3
"""Baut volt-lang-<version>.vsix ohne vsce (das braucht ein neueres Node): ZIP mit extension/, Manifest und Content-Types.
Aufruf im Ordner editors/vscode nach `npm install`."""
import json, os, subprocess, zipfile
from xml.sax.saxutils import escape

pkg = json.load(open("package.json"))
out = f"{pkg['name']}-{pkg['version']}.vsix"
prod = subprocess.run(["npm", "ls", "--omit=dev", "--all", "--parseable"], capture_output=True, text=True).stdout.split()
files = ["package.json", "extension.js", "language-configuration.json", "README.md", "syntaxes/volt.tmLanguage.json", "snippets/volt.json"]
for d in prod:
    if os.path.isdir(d) and d != os.getcwd():
        for root, _, names in os.walk(d):
            files += [os.path.relpath(os.path.join(root, n)) for n in names]
manifest = f'''<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
  <Metadata>
    <Identity Language="en-US" Id="{pkg['name']}" Version="{pkg['version']}" Publisher="{pkg['publisher']}"/>
    <DisplayName>{escape(pkg['displayName'])}</DisplayName>
    <Description xml:space="preserve">{escape(pkg['description'])}</Description>
    <Tags>volt,vt,vts</Tags>
    <Categories>{','.join(pkg['categories'])}</Categories>
    <GalleryFlags>Public</GalleryFlags>
    <Properties>
      <Property Id="Microsoft.VisualStudio.Code.Engine" Value="{pkg['engines']['vscode']}"/>
      <Property Id="Microsoft.VisualStudio.Code.ExtensionDependencies" Value=""/>
      <Property Id="Microsoft.VisualStudio.Code.ExtensionPack" Value=""/>
      <Property Id="Microsoft.VisualStudio.Code.ExtensionKind" Value="workspace"/>
    </Properties>
  </Metadata>
  <Installation><InstallationTarget Id="Microsoft.VisualStudio.Code"/></Installation>
  <Dependencies/>
  <Assets>
    <Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true"/>
    <Asset Type="Microsoft.VisualStudio.Services.Content.Details" Path="extension/README.md" Addressable="true"/>
  </Assets>
</PackageManifest>'''
types = '<?xml version="1.0" encoding="utf-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/Content-Types"><Default Extension=".json" ContentType="application/json"/><Default Extension=".js" ContentType="application/javascript"/><Default Extension=".md" ContentType="text/markdown"/><Default Extension=".vsixmanifest" ContentType="text/xml"/><Default Extension=".txt" ContentType="text/plain"/><Default Extension=".ts" ContentType="text/plain"/><Default Extension=".map" ContentType="application/json"/></Types>'
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", types)
    z.writestr("extension.vsixmanifest", manifest)
    for f in sorted(set(files)):
        z.write(f, "extension/" + f)
print(out, len(set(files)), "Dateien")
