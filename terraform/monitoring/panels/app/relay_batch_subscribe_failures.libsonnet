local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Relay Batch Subscribe Errors',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - Failed to batch subscribe to relay topic' % { env: vars.environment },
      message       = '%(env)s - Failed to batch subscribe to relay topic' % { env: vars.environment },
      notifications = vars.notifications,
      noDataState   = 'no_data',
      period        = '0m',
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 0 ],
          evaluatorType   = 'gt',
          operatorType    = 'or',
          queryRefId      = 'RelayBatchSubscribePermanentFailures',
          queryTimeStart  = '5m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (increase(relay_batch_subscribe_failures_total{is_permanent="true"}[$__rate_interval]))',
      legendFormat  = 'Permanent r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayBatchSubscribePermanentFailures',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (increase(relay_batch_subscribe_failures_total{is_permanent="false"}[$__rate_interval]))',
      legendFormat  = 'Temporary r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayBatchSubscribeTemporaryFailures',
    ))
}
