{{/*
Expand the name of the chart.
*/}}
{{- define "rate-limiter.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Fully qualified app name.  Truncated at 63 chars (DNS label limit).
*/}}
{{- define "rate-limiter.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Chart label value (name-version).
*/}}
{{- define "rate-limiter.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels applied to every resource.
*/}}
{{- define "rate-limiter.labels" -}}
helm.sh/chart: {{ include "rate-limiter.chart" . }}
{{ include "rate-limiter.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels (stable — used in matchLabels and Services).
*/}}
{{- define "rate-limiter.selectorLabels" -}}
app.kubernetes.io/name: {{ include "rate-limiter.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/* ── etcd helpers ─────────────────────────────────────────────────────────── */}}

{{/*
Name of the etcd StatefulSet and headless Service.
*/}}
{{- define "rate-limiter.etcdName" -}}
{{- printf "%s-etcd" (include "rate-limiter.fullname" .) }}
{{- end }}

{{/*
Generates the --initial-cluster bootstrap string from the replica count.
Each entry is  <pod-name>=http://<pod-name>.<svc>.<ns>.svc.cluster.local:2380
*/}}
{{- define "rate-limiter.etcdInitialCluster" -}}
{{- $etcdName := include "rate-limiter.etcdName" . -}}
{{- $ns       := .Release.Namespace -}}
{{- $peers    := list -}}
{{- range $i := until (int .Values.etcd.replicas) -}}
{{- $peer := printf "%s-%d=http://%s-%d.%s.%s.svc.cluster.local:2380" $etcdName $i $etcdName $i $etcdName $ns -}}
{{- $peers = append $peers $peer -}}
{{- end -}}
{{- join "," $peers -}}
{{- end }}

{{/*
Renders the etcd endpoints list as YAML lines for embedding in config.yaml.
*/}}
{{- define "rate-limiter.etcdEndpoints" -}}
{{- $etcdName := include "rate-limiter.etcdName" . -}}
{{- $ns       := .Release.Namespace -}}
{{- range $i := until (int .Values.etcd.replicas) -}}
- "http://{{ $etcdName }}-{{ $i }}.{{ $etcdName }}.{{ $ns }}.svc.cluster.local:2379"
{{ end -}}
{{- end }}

{{/* ── HMAC secret helper ───────────────────────────────────────────────────── */}}

{{/*
Name of the Secret that holds RATE_LIMIT_HMAC_SECRET.
Uses an existing secret when existingSecret is set; otherwise the chart-managed one.
*/}}
{{- define "rate-limiter.hmacSecretName" -}}
{{- if .Values.server.strictSecurity.existingSecret -}}
{{- .Values.server.strictSecurity.existingSecret -}}
{{- else -}}
{{- printf "%s-hmac" (include "rate-limiter.fullname" .) -}}
{{- end -}}
{{- end }}
