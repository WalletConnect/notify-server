#!/usr/bin/env bash
set -e

TERRAFORM_DIR="$(dirname "$0")"

accountId="$(aws sts get-caller-identity | jq -r .Account)"
region=$AWS_REGION

imageRepo="$accountId.dkr.ecr.$region.amazonaws.com/notify"

aws ecr get-login-password --region eu-central-1 | docker login --username AWS --password-stdin "$imageRepo"
# --platform=linux/amd64: Must target linux/amd64 as that is what ECS runs.
docker build "$TERRAFORM_DIR/.." -t "$imageRepo" --build-arg=release=true --platform=linux/amd64 $BUILD_ARGS
imageVersion="$(docker inspect --format="{{ .Id }}" "$imageRepo" | cut -d: -f2)"
tag="$imageRepo:$imageVersion"
docker tag "$imageRepo" "$tag"
docker push "$tag"

# TF_VAR_* env vars not supported for remote deployments, so use *.auto.tfvars instead which works
autoTfVars="$TERRAFORM_DIR/dev.auto.tfvars"
echo "image_version=\"$imageVersion\"" > "$autoTfVars"
echo "grafana_auth=\"$GRAFANA_AUTH\"" >> "$autoTfVars"

terraform -chdir="$TERRAFORM_DIR" workspace select wl-dev
terraform -chdir="$TERRAFORM_DIR" apply -auto-approve
