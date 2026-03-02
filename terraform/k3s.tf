# =============================================================================
# Linux / WSL2 / macOS resources (k3s native)
# =============================================================================

# Verify k3s binary exists
resource "terraform_data" "k3s_binary_check" {
  count = var.platform != "windows" ? 1 : 0

  input = var.k3s_binary_path

  provisioner "local-exec" {
    interpreter = ["bash", "-c"]
    command     = "test -x ${var.k3s_binary_path} && ${var.k3s_binary_path} --version"
  }
}

# Ensure k3s service is running
resource "terraform_data" "k3s_service" {
  count      = var.platform != "windows" ? 1 : 0
  depends_on = [terraform_data.k3s_binary_check]

  input = var.manage_service ? "managed" : "external"

  provisioner "local-exec" {
    interpreter = ["bash", "-c"]
    command     = <<-EOT
      if systemctl is-active --quiet k3s; then
        echo "K3s service is already running"
      else
        echo "Starting K3s service..."
        sudo systemctl start k3s
        echo "Waiting for K3s to be ready..."
        timeout 120 bash -c 'until kubectl get nodes 2>/dev/null | grep -q " Ready "; do sleep 2; done'
        echo "K3s is ready"
      fi
    EOT
  }

  provisioner "local-exec" {
    when        = destroy
    interpreter = ["bash", "-c"]
    command     = <<-EOT
      echo "Stopping K3s service..."
      sudo systemctl stop k3s || true
      echo "K3s service stopped"
    EOT
  }
}

# Copy kubeconfig for non-root access
resource "terraform_data" "kubeconfig_setup" {
  count      = var.platform != "windows" ? 1 : 0
  depends_on = [terraform_data.k3s_service]

  provisioner "local-exec" {
    interpreter = ["bash", "-c"]
    command     = <<-EOT
      KUBECONFIG_DIR=$(dirname ${var.kubeconfig_output_path})
      mkdir -p "$KUBECONFIG_DIR"
      sudo cp ${var.kubeconfig_path} ${var.kubeconfig_output_path}
      sudo chown $(id -u):$(id -g) ${var.kubeconfig_output_path}
      chmod 600 ${var.kubeconfig_output_path}
      echo "Kubeconfig copied to ${var.kubeconfig_output_path}"
    EOT
  }
}

# =============================================================================
# Windows resources (k3d — k3s-in-Docker)
# =============================================================================

# Create k3d cluster (k3s running inside Docker Desktop)
resource "terraform_data" "k3d_cluster_create" {
  count = var.platform == "windows" ? 1 : 0

  provisioner "local-exec" {
    interpreter = ["powershell", "-Command"]
    command     = <<-EOT
      $existing = k3d cluster list -o json | ConvertFrom-Json | Where-Object { $_.name -eq "claude-code" }
      if ($existing) {
        Write-Host "k3d cluster 'claude-code' already exists"
      } else {
        Write-Host "Creating k3d cluster..."
        k3d cluster create claude-code --wait --timeout 120s
        Write-Host "k3d cluster created"
      }
    EOT
  }

  provisioner "local-exec" {
    when        = destroy
    interpreter = ["powershell", "-Command"]
    command     = <<-EOT
      Write-Host "Deleting k3d cluster..."
      k3d cluster delete claude-code
      Write-Host "k3d cluster deleted"
    EOT
  }
}

# =============================================================================
# Shared: cluster health check (works on all platforms)
# =============================================================================

resource "terraform_data" "cluster_health_check" {
  depends_on = [
    terraform_data.kubeconfig_setup,
    terraform_data.k3d_cluster_create,
  ]

  provisioner "local-exec" {
    command = "kubectl cluster-info && kubectl get nodes"
  }
}
