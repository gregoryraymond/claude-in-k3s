# Verify k3s binary exists
resource "terraform_data" "k3s_binary_check" {
  input = var.k3s_binary_path

  provisioner "local-exec" {
    command = "test -x ${var.k3s_binary_path} && ${var.k3s_binary_path} --version"
  }
}

# Ensure k3s service is running
resource "terraform_data" "k3s_service" {
  depends_on = [terraform_data.k3s_binary_check]

  input = var.manage_service ? "managed" : "external"

  provisioner "local-exec" {
    command = <<-EOT
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
    when    = destroy
    command = <<-EOT
      echo "Stopping K3s service..."
      sudo systemctl stop k3s || true
      echo "K3s service stopped"
    EOT
  }
}

# Copy kubeconfig for non-root access
resource "terraform_data" "kubeconfig_setup" {
  depends_on = [terraform_data.k3s_service]

  provisioner "local-exec" {
    command = <<-EOT
      KUBECONFIG_DIR=$(dirname ${var.kubeconfig_output_path})
      mkdir -p "$KUBECONFIG_DIR"
      sudo cp ${var.kubeconfig_path} ${var.kubeconfig_output_path}
      sudo chown $(id -u):$(id -g) ${var.kubeconfig_output_path}
      chmod 600 ${var.kubeconfig_output_path}
      echo "Kubeconfig copied to ${var.kubeconfig_output_path}"
    EOT
  }
}

# Verify cluster health
resource "terraform_data" "cluster_health_check" {
  depends_on = [terraform_data.kubeconfig_setup]

  provisioner "local-exec" {
    command = <<-EOT
      echo "Verifying cluster health..."
      kubectl cluster-info
      kubectl get nodes
      echo "Cluster is healthy"
    EOT
  }
}
